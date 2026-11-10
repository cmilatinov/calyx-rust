use chrono::Local;
use env_filter::Filter;
use log::{Metadata, Record, SetLoggerError};
use std::collections::{HashSet, VecDeque};
use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use typed_builder::TypedBuilder;

const LOG_HISTORY_LIMIT: usize = 1000;
static LOG_HISTORY: OnceLock<Mutex<VecDeque<LogEntry>>> = OnceLock::new();

static WARNED_KEYS: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();

/// Logs a warning the first time `key` is seen, so per-frame problems do not flood the log.
pub fn warn_once(key: impl Into<String>, message: impl FnOnce() -> String) {
    let warned = WARNED_KEYS.get_or_init(Default::default);
    let first = warned
        .lock()
        .map(|mut keys| keys.insert(key.into()))
        .unwrap_or(false);
    if first {
        log::warn!("{}", message());
    }
}

/// Whether [`warn_once`] has warned under `key`.
#[cfg(test)]
pub(crate) fn has_warned(key: &str) -> bool {
    WARNED_KEYS
        .get()
        .and_then(|warned| warned.lock().ok().map(|keys| keys.contains(key)))
        .unwrap_or(false)
}

/// One log entry retained for editor console display.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LogEntry {
    pub timestamp: String,
    pub level: log::Level,
    pub target: String,
    pub message: String,
}

impl LogEntry {
    fn from_record(record: &Record) -> Self {
        Self {
            timestamp: log_timestamp(),
            level: record.level(),
            target: record.target().to_string(),
            message: record.args().to_string(),
        }
    }

    pub fn line(&self) -> String {
        format!(
            "{} {:<5} {} - {}",
            self.timestamp, self.level, self.target, self.message
        )
    }
}

/// Returns a snapshot of recent process log entries.
pub fn recent_log_entries() -> Vec<LogEntry> {
    LOG_HISTORY
        .get_or_init(Default::default)
        .lock()
        .map(|history| history.iter().cloned().collect())
        .unwrap_or_default()
}

fn push_log_entry(entry: LogEntry) {
    if let Ok(mut history) = LOG_HISTORY.get_or_init(Default::default).lock() {
        if history.len() >= LOG_HISTORY_LIMIT {
            history.pop_front();
        }
        history.push_back(entry);
    }
}

/// Initializes the global logger using a concrete logger implementation.
pub struct Log<T: LoggerImplementation> {
    logger: T,
}

impl<T: LoggerImplementation> Log<T> {
    /// Builds and installs the process-wide logger.
    pub fn new(logger: T) -> Self {
        let config = FilterConfig::from_env();
        let filter = config.filter;
        let max_level = filter.filter();
        let log_file_path = logger.log_file_path();
        let installed = match logger.log_file(&log_file_path) {
            Ok(file) => {
                let installed =
                    install_logger(MultiSinkLogger::new(filter, file), max_level).is_ok();
                if installed {
                    log::info!(
                        "Logging initialized; max_level={max_level}, file={}",
                        log_file_path.display()
                    );
                }
                installed
            }
            Err(error) => {
                let stdout_logger = StdoutLogger::new(filter);
                let installed = install_logger(stdout_logger, max_level).is_ok();
                if installed {
                    log::error!("Failed to create file log sink: {error}");
                }
                installed
            }
        };
        if installed {
            for (variable, spec) in &config.applied {
                log::info!("Log filter from {variable}: {spec}");
            }
            for (variable, spec) in &config.rejected {
                log::warn!("Ignoring {variable}={spec}: it is not a valid log filter");
            }
        }
        Self { logger }
    }

    /// Returns the logger configuration used for initialization.
    pub fn logger(&self) -> &T {
        &self.logger
    }
}

/// Supplies log output destinations for [`Log`].
pub trait LoggerImplementation {
    /// Returns the directory where log files should be created.
    fn log_dir(&self) -> io::Result<PathBuf>;
    /// Returns the full file path used by the file sink.
    fn log_file_path(&self) -> PathBuf;
    /// Returns the log file to write to.
    fn log_file(&self, path: &Path) -> io::Result<File>;
}

/// Default filesystem-backed logger configuration.
#[derive(TypedBuilder)]
pub struct DefaultLogger {
    #[builder]
    app_vendor: &'static str,
    #[builder]
    app_name: &'static str,
}

impl LoggerImplementation for DefaultLogger {
    fn log_dir(&self) -> io::Result<PathBuf> {
        let dir = std::env::current_dir()?.join("logs");
        std::fs::create_dir_all(&dir)?;
        Ok(dir)
    }

    fn log_file_path(&self) -> PathBuf {
        let app_name = self.app_name.to_ascii_lowercase();
        let app_vendor = self.app_vendor.to_ascii_lowercase();
        let file_name = format!("{app_vendor}_{app_name}_{}.log", file_timestamp());
        self.log_dir()
            .unwrap_or_else(|_| PathBuf::from("logs"))
            .join(file_name)
    }

    fn log_file(&self, path: &Path) -> io::Result<File> {
        OpenOptions::new().create(true).append(true).open(path)
    }
}

struct MultiSinkLogger {
    filter: Filter,
    file: Mutex<File>,
}

impl MultiSinkLogger {
    fn new(filter: Filter, file: File) -> Self {
        Self {
            filter,
            file: Mutex::new(file),
        }
    }
}

impl log::Log for MultiSinkLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        self.filter.enabled(metadata)
    }

    fn log(&self, record: &Record) {
        if !self.filter.matches(record) {
            return;
        }
        push_log_entry(LogEntry::from_record(record));
        let stdout_line = format_stdout_record(record);
        let _ = io::stdout().lock().write_all(stdout_line.as_bytes());
        if let Ok(mut file) = self.file.lock() {
            let line = format_file_record(record);
            let _ = file.write_all(line.as_bytes());
        }
    }

    fn flush(&self) {
        let _ = io::stdout().lock().flush();
        if let Ok(mut file) = self.file.lock() {
            let _ = file.flush();
        }
    }
}

struct StdoutLogger {
    filter: Filter,
}

impl StdoutLogger {
    fn new(filter: Filter) -> Self {
        Self { filter }
    }
}

impl log::Log for StdoutLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        self.filter.enabled(metadata)
    }

    fn log(&self, record: &Record) {
        if self.filter.matches(record) {
            push_log_entry(LogEntry::from_record(record));
            let _ = io::stdout()
                .lock()
                .write_all(format_stdout_record(record).as_bytes());
        }
    }

    fn flush(&self) {
        let _ = io::stdout().lock().flush();
    }
}

fn install_logger(
    logger: impl log::Log + 'static,
    max_level: log::LevelFilter,
) -> Result<(), SetLoggerError> {
    log::set_boxed_logger(Box::new(logger)).map(|_| {
        log::set_max_level(max_level);
    })
}

/// The environment variables read for log filter directives, lowest
/// precedence first. Each one is layered over [`DEFAULT_FILTER_SPEC`] and
/// the variables before it, as described on [`FilterConfig::build`].
const FILTER_ENV: [&str; 2] = ["RUST_LOG", "CALYX_LOG"];

/// The directives every Calyx process starts from.
const DEFAULT_FILTER_SPEC: &str = "calyx_engine=info,calyx_editor=info,sandbox=info";

/// The log filter a process installs, with the environment directives that
/// shaped it, logged once the logger is installed.
struct FilterConfig {
    filter: Filter,
    /// Variables whose directives were layered in, with their values.
    applied: Vec<(&'static str, String)>,
    /// Variables whose values did not parse and were ignored.
    rejected: Vec<(&'static str, String)>,
}

impl FilterConfig {
    fn from_env() -> Self {
        let specs = FILTER_ENV.map(|variable| (variable, std::env::var(variable).ok()));
        Self::build(&specs)
    }

    /// Layers each `(variable, spec)` over [`DEFAULT_FILTER_SPEC`] in order,
    /// so a later layer takes precedence over an earlier one.
    ///
    /// A directive naming a target (`calyx_engine=debug`) replaces the
    /// earlier directive for the same target and keeps the others, so a
    /// layer only changes the targets it names. A bare level (`off`, `warn`)
    /// sets the level of targets that no directive names, such as
    /// `wgpu_core`, and leaves the Calyx targets of the defaults at theirs:
    /// a machine-wide `RUST_LOG=off,...` set for other tools does not hide
    /// engine records. Name a Calyx target to change its level
    /// (`CALYX_LOG=calyx_engine=off`). An empty or unset variable adds
    /// nothing, and one that does not parse is ignored as a whole.
    fn build(specs: &[(&'static str, Option<String>)]) -> Self {
        let mut builder = env_filter::Builder::new();
        builder.parse(DEFAULT_FILTER_SPEC);
        let mut applied = Vec::new();
        let mut rejected = Vec::new();
        for (variable, spec) in specs {
            let Some(spec) = spec.as_deref().map(str::trim) else {
                continue;
            };
            if spec.is_empty() {
                continue;
            }
            // `try_parse` changes nothing when the spec does not parse.
            if builder.try_parse(spec).is_ok() {
                applied.push((*variable, spec.to_string()));
            } else {
                rejected.push((*variable, spec.to_string()));
            }
        }
        Self {
            filter: builder.build(),
            applied,
            rejected,
        }
    }
}

fn format_stdout_record(record: &Record) -> String {
    let level = format!("{:<5}", record.level());
    format!(
        "{} {}{}{} {} - {}\n",
        log_timestamp(),
        level_color(record.level()),
        level,
        "\x1b[0m",
        record.target(),
        record.args()
    )
}

fn format_file_record(record: &Record) -> String {
    format!(
        "{} {:<5} {} - {}\n",
        log_timestamp(),
        record.level(),
        record.target(),
        record.args()
    )
}

fn level_color(level: log::Level) -> &'static str {
    match level {
        log::Level::Error => "\x1b[1;31m",
        log::Level::Warn => "\x1b[33m",
        log::Level::Info => "\x1b[32m",
        log::Level::Debug => "\x1b[34m",
        log::Level::Trace => "\x1b[90m",
    }
}

fn log_timestamp() -> String {
    Local::now().format("%Y-%m-%d %H:%M:%S%.3f").to_string()
}

fn file_timestamp() -> String {
    Local::now().format("%Y%m%d_%H%M%S%.3f").to_string()
}

#[cfg(test)]
mod tests {
    use super::{format_file_record, format_stdout_record, FilterConfig, LogEntry};
    use env_filter::Filter;
    use log::{Level, LevelFilter, Record};

    fn record(target: &'static str, level: Level) -> Record<'static> {
        Record::builder()
            .args(format_args!("test"))
            .level(level)
            .target(target)
            .build()
    }

    fn filter(rust_log: Option<&str>, calyx_log: Option<&str>) -> Filter {
        FilterConfig::build(&[
            ("RUST_LOG", rust_log.map(String::from)),
            ("CALYX_LOG", calyx_log.map(String::from)),
        ])
        .filter
    }

    /// Asserts the engine, editor and sandbox targets log `info` and above.
    fn assert_calyx_targets_at_info(filter: &Filter) {
        for target in [
            "calyx_engine::runtime::player",
            "calyx_editor::remote",
            "sandbox::hud",
        ] {
            for level in [Level::Error, Level::Warn, Level::Info] {
                assert!(
                    filter.matches(&record(target, level)),
                    "{target} {level} is filtered out"
                );
            }
            assert!(!filter.matches(&record(target, Level::Debug)));
        }
    }

    #[test]
    fn default_filter_includes_calyx_targets_only() {
        let filter = filter(None, None);

        assert_eq!(filter.filter(), LevelFilter::Info);
        assert_calyx_targets_at_info(&filter);
        assert!(!filter.matches(&record("wgpu_core", Level::Info)));
    }

    #[test]
    fn empty_variables_keep_the_defaults() {
        let filter = filter(Some(""), Some("  "));

        assert_calyx_targets_at_info(&filter);
    }

    #[test]
    fn a_bare_rust_log_level_leaves_calyx_targets_at_their_level() {
        // This machine's user environment, set for other tools.
        let filter = filter(
            Some("off,bitwatch=trace,engine=trace,sandbox=trace,editor=trace"),
            None,
        );

        for target in ["calyx_engine::runtime::player", "calyx_editor::remote"] {
            for level in [Level::Error, Level::Warn, Level::Info] {
                assert!(filter.matches(&record(target, level)));
            }
            assert!(!filter.matches(&record(target, Level::Debug)));
        }
        assert!(filter.matches(&record("sandbox::hud", Level::Trace)));
        assert!(filter.matches(&record("bitwatch", Level::Trace)));
        assert!(!filter.matches(&record("wgpu_core", Level::Error)));
    }

    #[test]
    fn rust_log_target_directives_change_only_the_targets_they_name() {
        let filter = filter(Some("calyx_engine=trace,wgpu_core=warn"), None);

        assert_eq!(filter.filter(), LevelFilter::Trace);
        assert!(filter.matches(&record("calyx_engine::assets", Level::Trace)));
        assert!(filter.matches(&record("calyx_editor", Level::Info)));
        assert!(!filter.matches(&record("calyx_editor", Level::Debug)));
        assert!(filter.matches(&record("wgpu_core", Level::Warn)));
        assert!(!filter.matches(&record("wgpu_core", Level::Info)));
    }

    #[test]
    fn calyx_log_takes_precedence_over_rust_log() {
        let filter = filter(
            Some("calyx_engine=trace,wgpu_core=warn"),
            Some("calyx_engine=warn,calyx_editor=debug"),
        );

        assert!(!filter.matches(&record("calyx_engine::assets", Level::Info)));
        assert!(filter.matches(&record("calyx_engine::assets", Level::Warn)));
        assert!(filter.matches(&record("calyx_editor::remote", Level::Debug)));
        assert!(filter.matches(&record("sandbox::hud", Level::Info)));
        assert!(filter.matches(&record("wgpu_core", Level::Warn)));
    }

    #[test]
    fn naming_a_calyx_target_can_silence_it() {
        let filter = filter(None, Some("calyx_engine=off"));

        assert!(!filter.matches(&record("calyx_engine::assets", Level::Error)));
        assert!(filter.matches(&record("calyx_editor", Level::Info)));
    }

    #[test]
    fn an_invalid_variable_is_ignored_and_reported() {
        let config = FilterConfig::build(&[
            ("RUST_LOG", Some("calyx_engine=loud".to_string())),
            ("CALYX_LOG", Some("calyx_editor=debug".to_string())),
        ]);

        assert_eq!(
            config.rejected,
            [("RUST_LOG", "calyx_engine=loud".to_string())]
        );
        assert_eq!(
            config.applied,
            [("CALYX_LOG", "calyx_editor=debug".to_string())]
        );
        assert!(config
            .filter
            .matches(&record("calyx_engine::assets", Level::Info)));
        assert!(config
            .filter
            .matches(&record("calyx_editor::remote", Level::Debug)));
    }

    #[test]
    fn stdout_record_format_includes_level_color() {
        let line = format_stdout_record(&record("calyx_engine::assets", Level::Warn));

        assert!(line.contains("\x1b[33mWARN "));
        assert!(line.contains("\x1b[0m calyx_engine::assets - test"));
    }

    #[test]
    fn file_record_format_omits_level_color() {
        let line = format_file_record(&record("calyx_engine::assets", Level::Warn));

        assert!(!line.contains("\x1b["));
        assert!(line.contains("WARN  calyx_engine::assets - test"));
    }

    #[test]
    fn log_entry_line_uses_plain_console_format() {
        let entry = LogEntry {
            timestamp: "2026-05-13 00:00:00.000".to_string(),
            level: Level::Info,
            target: "calyx_editor".to_string(),
            message: "ready".to_string(),
        };

        assert_eq!(
            entry.line(),
            "2026-05-13 00:00:00.000 INFO  calyx_editor - ready"
        );
    }
}

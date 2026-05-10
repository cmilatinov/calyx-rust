use chrono::Local;
use env_filter::Filter;
use log::{Metadata, Record, SetLoggerError};
use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use typed_builder::TypedBuilder;

/// Initializes the global logger using a concrete logger implementation.
pub struct Log<T: LoggerImplementation> {
    logger: T,
}

impl<T: LoggerImplementation> Log<T> {
    /// Builds and installs the process-wide logger.
    pub fn new(logger: T) -> Self {
        let filter = configured_filter();
        let max_level = filter.filter();
        let log_file_path = logger.log_file_path();
        match logger.log_file(&log_file_path) {
            Ok(file) => {
                if install_logger(MultiSinkLogger::new(filter, file), max_level).is_ok() {
                    log::info!(
                        "Logging initialized; max_level={max_level}, file={}",
                        log_file_path.display()
                    );
                }
            }
            Err(error) => {
                let stdout_logger = StdoutLogger::new(filter);
                if install_logger(stdout_logger, max_level).is_ok() {
                    log::error!("Failed to create file log sink: {error}");
                }
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

fn configured_filter() -> Filter {
    let spec = std::env::var("CALYX_LOG")
        .or_else(|_| std::env::var("RUST_LOG"))
        .ok();
    build_filter(spec.as_deref())
}

fn build_filter(spec: Option<&str>) -> Filter {
    let mut builder = env_filter::Builder::new();
    match spec {
        Some(spec) if !spec.trim().is_empty() => {
            if builder.try_parse(spec).is_err() {
                builder = env_filter::Builder::new();
                builder.parse(default_filter_spec());
            }
        }
        _ => {
            builder.parse(default_filter_spec());
        }
    }
    builder.build()
}

fn default_filter_spec() -> &'static str {
    "engine=info,editor=info,sandbox=info"
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
    use super::{build_filter, format_file_record, format_stdout_record};
    use log::{Level, LevelFilter, Record};

    fn record(target: &'static str, level: Level) -> Record<'static> {
        Record::builder()
            .args(format_args!("test"))
            .level(level)
            .target(target)
            .build()
    }

    #[test]
    fn default_filter_includes_calyx_targets_only() {
        let filter = build_filter(None);

        assert_eq!(filter.filter(), LevelFilter::Info);
        assert!(filter.matches(&record("engine::assets", Level::Info)));
        assert!(filter.matches(&record("editor::project_manager", Level::Info)));
        assert!(!filter.matches(&record("wgpu_core", Level::Info)));
        assert!(!filter.matches(&record("engine::assets", Level::Debug)));
    }

    #[test]
    fn env_filter_respects_target_directives() {
        let filter = build_filter(Some("engine=trace,wgpu_core=warn"));

        assert_eq!(filter.filter(), LevelFilter::Trace);
        assert!(filter.matches(&record("engine::assets", Level::Trace)));
        assert!(filter.matches(&record("wgpu_core", Level::Warn)));
        assert!(!filter.matches(&record("wgpu_core", Level::Info)));
        assert!(!filter.matches(&record("editor", Level::Info)));
    }

    #[test]
    fn stdout_record_format_includes_level_color() {
        let line = format_stdout_record(&record("engine::assets", Level::Warn));

        assert!(line.contains("\x1b[33mWARN "));
        assert!(line.contains("\x1b[0m engine::assets - test"));
    }

    #[test]
    fn file_record_format_omits_level_color() {
        let line = format_file_record(&record("engine::assets", Level::Warn));

        assert!(!line.contains("\x1b["));
        assert!(line.contains("WARN  engine::assets - test"));
    }
}

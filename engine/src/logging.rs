use log::{Level, LevelFilter, Metadata, Record, SetLoggerError};
use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};
use typed_builder::TypedBuilder;

/// Initializes the global logger using a concrete logger implementation.
pub struct Log<T: LoggerImplementation> {
    logger: T,
}

impl<T: LoggerImplementation> Log<T> {
    /// Builds and installs the process-wide logger.
    pub fn new(logger: T) -> Self {
        let level_filter = configured_level_filter();
        let log_file_path = logger.log_file_path();
        match logger.log_file(&log_file_path) {
            Ok(file) => {
                if install_logger(MultiSinkLogger::new(level_filter, file), level_filter).is_ok() {
                    log::info!(
                        "Logging initialized; level={level_filter}, file={}",
                        log_file_path.display()
                    );
                }
            }
            Err(error) => {
                let stdout_logger = StdoutLogger::new(level_filter);
                if install_logger(stdout_logger, level_filter).is_ok() {
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
        let file_name = format!("{app_vendor}_{app_name}_{}.log", timestamp_millis());
        self.log_dir()
            .unwrap_or_else(|_| PathBuf::from("logs"))
            .join(file_name)
    }

    fn log_file(&self, path: &Path) -> io::Result<File> {
        OpenOptions::new().create(true).append(true).open(path)
    }
}

struct MultiSinkLogger {
    level_filter: LevelFilter,
    file: Mutex<File>,
}

impl MultiSinkLogger {
    fn new(level_filter: LevelFilter, file: File) -> Self {
        Self {
            level_filter,
            file: Mutex::new(file),
        }
    }
}

impl log::Log for MultiSinkLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        level_enabled(metadata.level(), self.level_filter)
    }

    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let line = format_record(record);
        let _ = io::stdout().lock().write_all(line.as_bytes());
        if let Ok(mut file) = self.file.lock() {
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
    level_filter: LevelFilter,
}

impl StdoutLogger {
    fn new(level_filter: LevelFilter) -> Self {
        Self { level_filter }
    }
}

impl log::Log for StdoutLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        level_enabled(metadata.level(), self.level_filter)
    }

    fn log(&self, record: &Record) {
        if self.enabled(record.metadata()) {
            let _ = io::stdout()
                .lock()
                .write_all(format_record(record).as_bytes());
        }
    }

    fn flush(&self) {
        let _ = io::stdout().lock().flush();
    }
}

fn install_logger(
    logger: impl log::Log + 'static,
    level_filter: LevelFilter,
) -> Result<(), SetLoggerError> {
    log::set_boxed_logger(Box::new(logger)).map(|_| {
        log::set_max_level(level_filter);
    })
}

fn configured_level_filter() -> LevelFilter {
    let value = std::env::var("CALYX_LOG")
        .or_else(|_| std::env::var("RUST_LOG"))
        .unwrap_or_else(|_| "info".to_string());
    value
        .split(',')
        .filter_map(|directive| directive.rsplit('=').next())
        .filter_map(parse_level_filter)
        .max()
        .unwrap_or(LevelFilter::Info)
}

fn parse_level_filter(value: &str) -> Option<LevelFilter> {
    match value.trim().to_ascii_lowercase().as_str() {
        "off" => Some(LevelFilter::Off),
        "error" => Some(LevelFilter::Error),
        "warn" | "warning" => Some(LevelFilter::Warn),
        "info" => Some(LevelFilter::Info),
        "debug" => Some(LevelFilter::Debug),
        "trace" => Some(LevelFilter::Trace),
        _ => None,
    }
}

fn level_enabled(level: Level, level_filter: LevelFilter) -> bool {
    level_filter
        .to_level()
        .map(|max_level| level <= max_level)
        .unwrap_or(false)
}

fn format_record(record: &Record) -> String {
    format!(
        "{} {:<5} {} - {}\n",
        timestamp_millis(),
        record.level(),
        record.target(),
        record.args()
    )
}

fn timestamp_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::{level_enabled, parse_level_filter};
    use log::{Level, LevelFilter};

    #[test]
    fn parses_log_levels() {
        assert_eq!(parse_level_filter("trace"), Some(LevelFilter::Trace));
        assert_eq!(parse_level_filter("warning"), Some(LevelFilter::Warn));
        assert_eq!(parse_level_filter("unknown"), None);
    }

    #[test]
    fn filters_records_at_or_above_level() {
        assert!(level_enabled(Level::Error, LevelFilter::Info));
        assert!(level_enabled(Level::Info, LevelFilter::Info));
        assert!(!level_enabled(Level::Debug, LevelFilter::Info));
        assert!(!level_enabled(Level::Error, LevelFilter::Off));
    }
}

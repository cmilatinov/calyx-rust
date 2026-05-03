use env_logger::{Target, WriteStyle};
use log::LevelFilter;
use std::fs::File;
use std::path::PathBuf;
use typed_builder::TypedBuilder;

/// Initializes the global logger using a concrete logger implementation.
pub struct Log<T: LoggerImplementation> {
    #[allow(unused)]
    logger: T,
}

impl<T: LoggerImplementation> Log<T> {
    /// Builds and installs the process-wide logger.
    pub fn new(logger: T) -> Self {
        let mut builder =
            env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("off"));
        builder
            .target(Target::Stdout)
            .write_style(WriteStyle::Always);
        if let Some(file) = logger.log_file() {
            builder = env_logger::Builder::new();
            builder
                .filter_level(LevelFilter::Off)
                .filter_module(env!("CARGO_PKG_NAME"), LevelFilter::Trace)
                .target(Target::Pipe(Box::new(file)))
                .write_style(WriteStyle::Never);
        }
        builder.init();
        Self { logger }
    }
}

/// Supplies log output destinations for [`Log`].
pub trait LoggerImplementation {
    /// Returns the directory where log files should be created.
    fn log_dir(&self) -> std::io::Result<PathBuf>;
    /// Returns the log file to write to, or `None` to keep logging on stdout.
    fn log_file(&self) -> Option<File>;
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
    fn log_dir(&self) -> std::io::Result<PathBuf> {
        dirs::data_local_dir()
            .ok_or(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Application directory does not exist",
            ))
            .map(|mut dir| {
                dir.push(self.app_vendor);
                dir.push(self.app_name);
                dir
            })
            .and_then(|dir| {
                if !dir.exists() {
                    std::fs::create_dir_all(dir.as_path()).map(|_| dir)
                } else {
                    Ok(dir)
                }
            })
    }

    fn log_file(&self) -> Option<File> {
        #[cfg(not(feature = "log_file"))]
        {
            None
        }
        #[cfg(feature = "log_file")]
        {
            use std::fs::File;
            use std::time::SystemTime;
            let timestamp = SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap();
            let Ok(log_dir) = self.log_dir() else {
                return None;
            };
            let file_name = format!(
                "{}/{}_{}_{}.log",
                log_dir.display(),
                self.app_name,
                self.app_vendor,
                timestamp.as_millis()
            );
            File::create(file_name).ok()
        }
    }
}

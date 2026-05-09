use log::{error, Level, Log, Metadata, Record, SetLoggerError};
use serde::Serialize;
use std::error::Error;

/// Logs `err` and every [`Error::source`] in the chain (for example tokio-postgres
/// reports only "invalid configuration" while the cause is "password missing").
pub fn error_chain(err: &(dyn Error + 'static)) {
    error!("{}", err);
    log_error_sources(err.source());
}

/// Same as [`error_chain`], but prefixes the top-level message (e.g. context + primary error).
pub fn error_chain_with_prefix(prefix: &str, err: &(dyn Error + 'static)) {
    error!("{}: {}", prefix, err);
    log_error_sources(err.source());
}

fn log_error_sources(mut cur: Option<&(dyn Error + 'static)>) {
    while let Some(e) = cur {
        error!("caused by: {}", e);
        cur = e.source();
    }
}

struct JsonLogger;

const LOGGER: JsonLogger = JsonLogger;

impl Log for JsonLogger {
    fn enabled(&self, _: &Metadata) -> bool {
        true
    }

    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }

        #[derive(Serialize)]
        struct Msg {
            level: &'static str,
            ts: u128,
            msg: String,
            filename: Option<String>,
            line: Option<u32>,
        }

        let msg = Msg {
            level: level_str(record.level()),
            ts: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis(),
            msg: format!("{}", record.args()),
            filename: record.file().map(|s| s.to_string()),
            line: record.line(),
        };

        match serde_json::to_string(&msg) {
            Ok(line) => {
                if record.level() <= Level::Warn {
                    println!("{}", line);
                } else {
                    eprintln!("{}", line);
                }
            }
            Err(e) => {
                eprintln!("log marshal error: {}", e);
            }
        }
    }

    fn flush(&self) {}
}

fn level_str(level: Level) -> &'static str {
    match level {
        Level::Trace => "Trace",
        Level::Debug => "Debug",
        Level::Info => "Info",
        Level::Warn => "Warn",
        Level::Error => "Error",
    }
}

pub fn init_with_level(level: log::LevelFilter) -> Result<(), SetLoggerError> {
    log::set_logger(&LOGGER).map(|()| log::set_max_level(level))
}

pub fn init_from_env() -> Result<(), SetLoggerError> {
    let level = match std::env::var("RUST_LOG") {
        Err(_) => log::LevelFilter::Off,
        Ok(s) => match s.to_lowercase().as_str() {
            "trace" => log::LevelFilter::Trace,
            "debug" => log::LevelFilter::Debug,
            "info" => log::LevelFilter::Info,
            "warn" => log::LevelFilter::Warn,
            "error" => log::LevelFilter::Error,
            _ => log::LevelFilter::Off,
        },
    };

    init_with_level(level)
}

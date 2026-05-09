use log::{Level, Log, Metadata, Record, SetLoggerError};
use serde::Serialize;

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

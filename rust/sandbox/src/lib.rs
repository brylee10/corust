#![deny(dead_code)]

use ansi_term::Color;
use env_logger::Target;
use log::Level;
use std::io::Write;

pub(crate) mod codec;
pub mod container;
pub mod runner;

// Number of bytes reserved to store the message size. Prefixes every serialized message.
const MESSAGE_BUF_SIZE_BYTES: usize = 8;

pub fn init_logger(target: Target, log_level: String) {
    let log_level = match log_level.to_uppercase().as_str() {
        "ERROR" => log::LevelFilter::Error,
        "WARN" => log::LevelFilter::Warn,
        "INFO" => log::LevelFilter::Info,
        "DEBUG" => log::LevelFilter::Debug,
        "TRACE" => log::LevelFilter::Trace,
        _ => panic!("Invalid log level: {}", log_level),
    };
    env_logger::builder()
        .format(|buf, record| {
            let level = match record.level() {
                Level::Error => Color::Red.paint("ERROR"),
                Level::Warn => Color::Yellow.paint("WARN"),
                Level::Info => Color::Green.paint("INFO"),
                Level::Debug => Color::Blue.paint("DEBUG"),
                Level::Trace => Color::Purple.paint("TRACE"),
            };

            writeln!(
                buf,
                "[{} {}:{}] {}",
                level,
                record.file().unwrap_or("unknown"),
                record.line().unwrap_or(0),
                record.args()
            )
        })
        .filter_level(log_level)
        .target(target)
        .init();
}

#![deny(dead_code)]

use ansi_term::Color;
use env_logger::Target;
use log::Level;
use std::io::Write;

pub mod container;
pub mod runner;

// Number of bytes reserved to store the message size.
// Prefixes every message.
const MESSAGE_BUF_SIZE_BYTES: usize = 4;

pub fn init_logger(target: Target) {
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
        .target(target)
        .init();
}

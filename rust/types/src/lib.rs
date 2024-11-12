use std::fmt::{Display, Formatter};

use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone, Copy)]
pub enum TargetType {
    Library,
    Binary,
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy)]
pub enum CargoCommand {
    Build,
    Run,
    Test,
    Clippy,
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy)]
pub enum OptLevel {
    Debug,
    Release,
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy)]
pub enum Channel {
    Stable,
    Beta,
    Nightly,
}

impl Display for Channel {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Channel::Stable => write!(f, "stable"),
            Channel::Beta => write!(f, "beta"),
            Channel::Nightly => write!(f, "nightly"),
        }
    }
}

//! Fundamental execution configuration and message types, shared by both
//! the `corust_components` and `corust_runner` crates.

pub use container::{ContainerMessage, ContainerResponse};
pub use execution::{
    CargoCommand, Channel, CodeOutputState, ExecuteCommand, ExecuteResponse, OptLevel,
    RunnerOutput, TargetType,
};

pub mod container;
pub mod execution;
pub mod standalone;

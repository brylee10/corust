//! Sample client and server to illustrate and test a operational transform implementation.
//!
//! Client/Server algorithm inspired by Daniel Spiewak's blog on the implementation in Novell Pulse:
//! https://web.archive.org/web/20120107060932/http://www.codecommit.com/blog/java/understanding-and-applying-operational-transformation
#![deny(dead_code)]

mod messages;
pub use messages::*;
pub mod client;
pub mod network;
pub mod server;
mod web_utils;

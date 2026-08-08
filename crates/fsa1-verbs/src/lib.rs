// Concern: everything both front ends need to run a verb and draw its answer | Non-concern: argv, exit codes, JSON-RPC, help text | IO: (a path + options) -> an outcome, or a Refusal

pub mod address;
mod charts;
pub mod ops;
pub mod present;
pub mod refusal;

pub use charts::FigureNotDrawn;
pub use refusal::{Kind, Refusal};

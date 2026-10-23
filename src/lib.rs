//! Compile class-aware JSON Schemas for Rojo projects and models.

mod compiler;

pub use compiler::{Artifacts, ClientTrackerStats, Config, Stats, check, generate, write};

//! Generate and check Rojo schemas from source checkouts.

pub mod cli;

use std::process::ExitCode;

fn main() -> ExitCode {
    cli::run()
}

use std::{process::ExitCode, time::Instant};

use clap::Parser as _;
use eyre::Result;
use rojo_schema::{Artifacts, Config, check, generate, write};

use super::{
    arguments::{Arguments, Command},
    output,
};

/// Parse command-line arguments and execute the requested compilation operation.
#[must_use]
pub fn run() -> ExitCode {
    let arguments = Arguments::parse();
    let result = output::initialize(arguments.debug).and_then(|()| execute(arguments));

    match result {
        Ok(()) => ExitCode::SUCCESS,

        Err(error) => {
            output::failure(error);

            ExitCode::FAILURE
        }
    }
}

fn execute(arguments: Arguments) -> Result<()> {
    let started = Instant::now();
    let progress = output::progress(arguments.debug);

    let result: Result<(&str, Artifacts)> = (|| match arguments.command {
        Command::Generate(paths) => {
            let config = Config::try_from(paths)?;
            let artifacts = generate(&config)?;
            write(&config, &artifacts)?;

            Ok(("Generated", artifacts))
        }

        Command::Check(paths) => Ok(("Checked", check(&Config::try_from(paths)?)?)),
    })();

    progress.finish_and_clear();
    let (verb, artifacts) = result?;
    output::summary(verb, &artifacts, started.elapsed());

    Ok(())
}

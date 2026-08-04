use std::{path::PathBuf, time::Instant};

use anyhow::Result;
use clap::{Args, Parser, Subcommand};
use rojo_schema::{check, generate, write, Config};

#[derive(Debug, Parser)]
#[command(name = "rojo-schema", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Generate all deterministic schema artifacts.
    Generate(Paths),
    /// Regenerate twice and fail if artifacts are stale or nondeterministic.
    Check(Paths),
}

#[derive(Debug, Clone, Args)]
struct Paths {
    /// Rojo source checkout.
    #[arg(long)]
    rojo: PathBuf,

    /// Creator Docs checkout or its engine reference directory.
    #[arg(long)]
    docs: PathBuf,

    /// Generated Draft 2020-12 schema.
    #[arg(long, default_value = "dist/rojo.schema.json")]
    output: PathBuf,

    /// Generated source/version manifest.
    #[arg(long, default_value = "dist/manifest.json")]
    manifest: PathBuf,

    /// Generated API/reflection coverage report.
    #[arg(long, default_value = "dist/coverage.json")]
    coverage: PathBuf,
}

impl From<Paths> for Config {
    fn from(paths: Paths) -> Self {
        Self {
            rojo: paths.rojo,
            docs: paths.docs,
            output: paths.output,
            manifest: paths.manifest,
            coverage: paths.coverage,
        }
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let started = Instant::now();
    let (verb, artifacts) = match cli.command {
        Command::Generate(paths) => {
            let config = Config::from(paths);
            let artifacts = generate(&config)?;
            write(&config, &artifacts)?;
            ("generated", artifacts)
        }
        Command::Check(paths) => ("checked", check(&Config::from(paths))?),
    };

    eprintln!(
        "{verb} {} in {:.2?}: {} bytes, {} classes, {} flattened properties, {} enums, {} definitions, {} conflicts, {} unclassified",
        artifacts.schema_id,
        started.elapsed(),
        artifacts.stats.schema_bytes,
        artifacts.stats.classes,
        artifacts.stats.flattened_properties,
        artifacts.stats.enums,
        artifacts.stats.definitions,
        artifacts.stats.conflicts,
        artifacts.stats.unclassified,
    );
    Ok(())
}

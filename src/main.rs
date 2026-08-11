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

    /// `MaximumADHD` Roblox Client Tracker checkout on its `roblox` branch.
    #[arg(long)]
    tracker: PathBuf,

    /// Generated Draft 2020-12 project schema.
    #[arg(long, default_value = "dist/project.schema.json")]
    project: PathBuf,

    /// Generated Draft 2020-12 model schema.
    #[arg(long, default_value = "dist/model.schema.json")]
    model: PathBuf,

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
            tracker: paths.tracker,
            project: paths.project,
            model: paths.model,
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
        "{verb} {} and {} in {:.2?}: {} project bytes, {} model bytes, {} classes, {} flattened properties, {} enums, {}/{} definitions, {} conflicts, {} unclassified",
        artifacts.project_id,
        artifacts.model_id,
        started.elapsed(),
        artifacts.stats.project_schema_bytes,
        artifacts.stats.model_schema_bytes,
        artifacts.stats.classes,
        artifacts.stats.flattened_properties,
        artifacts.stats.enums,
        artifacts.stats.project_definitions,
        artifacts.stats.model_definitions,
        artifacts.stats.conflicts,
        artifacts.stats.unclassified,
    );
    Ok(())
}

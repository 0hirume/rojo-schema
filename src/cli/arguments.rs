use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};
use eyre::{Report, Result};
use rojo_schema::Config;

use super::source;

#[derive(Debug, Parser)]
#[command(name = "rojo-schema", version, about)]
pub(super) struct Arguments {
    #[arg(long, global = true, help = "Show diagnostic tracing")]
    pub debug: bool,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub(super) enum Command {
    #[command(about = "Generate all deterministic schema artifacts")]
    Generate(Paths),

    #[command(about = "Regenerate twice and fail if artifacts are stale or nondeterministic")]
    Check(Paths),
}

#[derive(Debug, Clone, Args)]
pub(super) struct Paths {
    #[arg(long, help = "Rojo source checkout")]
    rojo: Option<PathBuf>,

    #[arg(long, help = "Creator Docs checkout or its engine reference directory")]
    docs: Option<PathBuf>,

    #[arg(long, help = "Roblox Client Tracker checkout on its roblox branch")]
    tracker: Option<PathBuf>,

    #[arg(
        long,
        default_value = "dist/project.schema.json",
        help = "Generated project schema"
    )]
    project: PathBuf,

    #[arg(
        long,
        default_value = "dist/model.schema.json",
        help = "Generated model schema"
    )]
    model: PathBuf,

    #[arg(
        long,
        default_value = "dist/coverage.json",
        help = "Generated API coverage report"
    )]
    coverage: PathBuf,
}

impl TryFrom<Paths> for Config {
    type Error = Report;

    fn try_from(paths: Paths) -> Result<Self> {
        Ok(Self {
            rojo: source::load(
                paths.rojo,
                "rojo",
                "https://github.com/rojo-rbx/rojo.git",
                None,
                &[],
            )?,
            docs: source::load(
                paths.docs,
                "docs",
                "https://github.com/Roblox/creator-docs.git",
                None,
                &["/content/en-us/reference/engine/"],
            )?,
            tracker: source::load(
                paths.tracker,
                "tracker",
                "https://github.com/MaximumADHD/Roblox-Client-Tracker.git",
                Some("roblox"),
                &["/Full-API-Dump.json", "/version.txt"],
            )?,
            project: paths.project,
            model: paths.model,
            coverage: paths.coverage,
        })
    }
}

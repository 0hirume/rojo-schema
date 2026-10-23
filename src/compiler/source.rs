use std::{fs, path::Path};

use cargo_metadata::{Metadata, MetadataCommand, Package};
use eyre::{Context as _, ContextCompat as _, Result, ensure};
use rbx_reflection::ReflectionDatabase;

use super::grammar;
const REFLECTION_CRATE: &str = "rbx_reflection_database";

#[derive(Debug, Clone)]
pub(super) struct Rojo {
    pub version: String,
    pub grammar: grammar::Grammar,
}

pub(super) struct Reflection {
    version: String,
    bytes: Vec<u8>,
}

impl Reflection {
    pub(super) fn database(&self) -> Result<ReflectionDatabase<'_>> {
        rmp_serde::from_slice(&self.bytes).with_context(|| {
            format!(
                "decoding Rojo's {REFLECTION_CRATE} {} database with the generator's reflection types",
                self.version
            )
        })
    }
}

pub(super) fn load_rojo(path: &Path) -> Result<Rojo> {
    let manifest = path.join("Cargo.toml");
    let lockfile = path.join("Cargo.lock");
    let source = path.join("src");

    ensure!(
        manifest.is_file(),
        "Rojo manifest not found at {}",
        manifest.display()
    );

    ensure!(
        lockfile.is_file(),
        "Rojo lockfile not found at {}",
        lockfile.display()
    );

    ensure!(
        source.is_dir(),
        "Rojo source not found at {}",
        source.display()
    );

    let metadata = cargo_metadata(&manifest, true)?;

    let package = metadata
        .root_package()
        .context("Rojo manifest has no root package")?;

    let grammar = grammar::load(&source)?;

    Ok(Rojo {
        version: package.version.to_string(),
        grammar,
    })
}

pub(super) fn load_reflection(rojo: &Path) -> Result<Reflection> {
    let metadata = cargo_metadata(&rojo.join("Cargo.toml"), false)?;
    let package = package(&metadata, REFLECTION_CRATE)?;

    let database = package
        .manifest_path
        .parent()
        .context("reflection package manifest has no parent")?
        .join("database.msgpack");

    let bytes = fs::read(&database).with_context(|| format!("reading {database}"))?;

    Ok(Reflection {
        version: package.version.to_string(),
        bytes,
    })
}

fn cargo_metadata(manifest: &Path, no_dependencies: bool) -> Result<Metadata> {
    let mut command = MetadataCommand::new();

    command
        .manifest_path(manifest)
        .other_options(vec!["--locked".to_owned()]);

    if no_dependencies {
        command.no_deps();
    }

    command
        .exec()
        .with_context(|| format!("reading Cargo metadata for {}", manifest.display()))
}

fn package<'metadata>(metadata: &'metadata Metadata, name: &str) -> Result<&'metadata Package> {
    let matches = metadata
        .packages
        .iter()
        .filter(|package| package.name.as_str() == name)
        .collect::<Vec<_>>();

    ensure!(
        matches.len() == 1,
        "expected one resolved {name} package, found {}",
        matches.len()
    );

    Ok(matches[0])
}

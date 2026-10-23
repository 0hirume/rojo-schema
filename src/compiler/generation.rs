use std::{
    collections::BTreeMap,
    fs,
    io::{self, Write as _},
    path::{Path, PathBuf},
};

use eyre::{Context as _, ContextCompat as _, Result, bail};
use serde::Serialize;

use super::{
    api, docs, format, inventory,
    model::{Coverage, Stats},
    schema, source, tracker,
};

/// Source checkouts and output destinations for a compilation.
#[derive(Debug, Clone)]
pub struct Config {
    /// Rojo checkout containing its source and locked dependencies.
    pub rojo: PathBuf,

    /// Creator Docs checkout or engine reference directory.
    pub docs: PathBuf,

    /// Client Tracker directory containing its API dump and version.
    pub tracker: PathBuf,

    /// Destination for the project schema.
    pub project: PathBuf,

    /// Destination for the model schema.
    pub model: PathBuf,

    /// Destination for the API coverage report.
    pub coverage: PathBuf,
}

/// Generated UTF-8 JSON documents and their compilation statistics.
#[derive(Debug, Clone)]
pub struct Artifacts {
    /// Project schema bytes.
    pub project: Vec<u8>,

    /// Model schema bytes.
    pub model: Vec<u8>,

    /// API coverage report bytes.
    pub coverage: Vec<u8>,

    /// Counts collected during compilation.
    pub stats: Stats,

    /// Canonical project schema identifier.
    pub project_id: String,

    /// Canonical model schema identifier.
    pub model_id: String,
}

/// Compile all artifacts in memory from the configured source checkouts.
///
/// # Errors
///
/// Returns an error when a source is missing, malformed, incompatible with the
/// reflection types, or cannot be represented by the schema compiler.
#[tracing::instrument(skip_all, fields(rojo = %config.rojo.display(), docs = %config.docs.display(), tracker = %config.tracker.display()))]
pub fn generate(config: &Config) -> Result<Artifacts> {
    let rojo = source::load_rojo(&config.rojo)?;
    let docs = docs::load(&config.docs)?;
    let tracker = tracker::load(&config.tracker)?;
    let reflection = source::load_reflection(&config.rojo)?;
    let database = reflection.database()?;
    let formats = format::values(&database)?;
    let api = api::build(&docs, &tracker, &database, &formats.variants);
    let schemas = schema::build(&api, &rojo.version, &rojo.grammar, &formats, &docs)?;

    let project_id = schemas.project["$id"]
        .as_str()
        .context("generated project schema is missing $id")?
        .to_owned();

    let model_id = schemas.model["$id"]
        .as_str()
        .context("generated model schema is missing $id")?
        .to_owned();

    let project_bytes = serde_json::to_vec(&schemas.project)?;
    let model_bytes = serde_json::to_vec(&schemas.model)?;

    let property_count = api
        .classes
        .values()
        .map(|class| class.properties.len())
        .sum();

    let enum_items = api.enums.values().map(|item| item.items.len()).sum();

    let stats = Stats {
        classes: api.classes.len(),
        properties: property_count,
        flattened_properties: schemas.flattened_properties,
        enums: api.enums.len(),
        enum_items,
        project_definitions: schemas.project_definitions,
        model_definitions: schemas.model_definitions,
        variant_types: api.variant_types.len(),
        api_items: api.coverage.len(),
        conflicts: api.diagnostics.len(),
        unclassified: inventory::unclassified(
            &docs,
            &tracker,
            &database,
            &api.variant_types,
            &api.coverage,
        ),
        project_schema_bytes: project_bytes.len(),
        model_schema_bytes: model_bytes.len(),
        client_tracker: api.client_tracker.clone(),
    };

    let mut classifications = BTreeMap::new();

    for item in &api.coverage {
        *classifications.entry(item.classification).or_insert(0) += 1;
    }

    let coverage = Coverage {
        counts: stats.clone(),
        classifications,
        docs: api.docs_counts,
        variant_types: api.variant_types,
        deprecation_overrides: api.deprecation_overrides,
        diagnostics: api.diagnostics,
        client_tracker: api.client_tracker,
        items: api.coverage,
    };

    let coverage_bytes = pretty(&coverage)?;

    Ok(Artifacts {
        project: project_bytes,
        model: model_bytes,
        coverage: coverage_bytes,
        stats,
        project_id,
        model_id,
    })
}

/// Write generated artifacts to their configured output paths.
///
/// # Errors
///
/// Returns an error when an output directory or file cannot be created.
pub fn write(config: &Config, artifacts: &Artifacts) -> Result<()> {
    write_file(&config.project, &artifacts.project)?;
    write_file(&config.model, &artifacts.model)?;
    write_file(&config.coverage, &artifacts.coverage)?;

    Ok(())
}

/// Verify deterministic generation and compare it with the output files.
///
/// # Errors
///
/// Returns an error when generation fails, produces different bytes twice, or
/// any configured output is missing or stale.
pub fn check(config: &Config) -> Result<Artifacts> {
    let first = generate(config)?;
    let second = generate(config)?;

    if first.project != second.project
        || first.model != second.model
        || first.coverage != second.coverage
    {
        bail!("generation is nondeterministic for identical inputs");
    }

    let expected = [
        (&config.project, &first.project, "project schema"),
        (&config.model, &first.model, "model schema"),
        (&config.coverage, &first.coverage, "coverage"),
    ];

    let mut stale = Vec::new();

    for (path, bytes, name) in expected {
        match fs::read(path) {
            Ok(committed) if committed == *bytes => {}
            Ok(_) => stale.push(format!("{name} differs: {}", path.display())),
            Err(error) => stale.push(format!("{name} missing: {} ({error})", path.display())),
        }
    }

    if !stale.is_empty() {
        bail!("generated artifacts are stale:\n{}", stale.join("\n"));
    }

    Ok(first)
}

fn write_file(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));

    let permissions = match fs::symlink_metadata(path) {
        Ok(metadata) => {
            if !metadata.is_file() || metadata.permissions().readonly() {
                bail!("output is not a writable regular file: {}", path.display());
            }

            Some(metadata.permissions())
        }

        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => return Err(error).with_context(|| format!("reading {}", path.display())),
    };

    fs::create_dir_all(parent)
        .with_context(|| format!("creating output directory {}", parent.display()))?;

    let mut staged = tempfile::NamedTempFile::new_in(parent)
        .with_context(|| format!("staging {}", path.display()))?;

    staged.write_all(bytes)?;

    if let Some(permissions) = permissions {
        staged.as_file().set_permissions(permissions)?;
    }

    staged.as_file().sync_all()?;

    staged
        .persist(path)
        .with_context(|| format!("writing {}", path.display()))?;

    Ok(())
}

fn pretty(value: &impl Serialize) -> Result<Vec<u8>> {
    let mut bytes = serde_json::to_vec_pretty(value)?;
    bytes.push(b'\n');

    Ok(bytes)
}

mod api;
mod docs;
mod format;
mod grammar;
pub mod model;
mod pointer;
mod schema;
mod source;
mod tracker;

use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use anyhow::{bail, Context, Result};
use serde::Serialize;

use crate::model::{Classification, Coverage, Manifest, SourceInfo, Stats};

#[derive(Debug, Clone)]
pub struct Config {
    pub rojo: PathBuf,
    pub docs: PathBuf,
    pub tracker: PathBuf,
    pub project: PathBuf,
    pub model: PathBuf,
    pub manifest: PathBuf,
    pub coverage: PathBuf,
}

#[derive(Debug, Clone)]
pub struct Artifacts {
    pub project: Vec<u8>,
    pub model: Vec<u8>,
    pub manifest: Vec<u8>,
    pub coverage: Vec<u8>,
    pub stats: Stats,
    pub project_id: String,
    pub model_id: String,
}

/// Compile all artifacts in memory from the configured source checkouts.
///
/// # Errors
///
/// Returns an error when a source is missing, malformed, inconsistent with the
/// pinned reflection stack, or cannot be represented by the schema compiler.
pub fn generate(config: &Config) -> Result<Artifacts> {
    let rojo = source::load_rojo(&config.rojo)?;
    let docs = docs::load(&config.docs)?;
    let tracker = tracker::load(&config.tracker)?;
    let docs_source = source::docs_source(&config.docs, &docs.studio_version)?;
    let reflection_source = source::reflection_source(&config.rojo)?;
    let tracker_source = source::tracker_source(&config.tracker)?;
    let formats = format::values()?;
    let api = api::build(&docs, &tracker, &formats.variants);
    let schemas = schema::build(&api, &rojo.source.version, &rojo.grammar, &formats)?;
    let project_id = schemas.project["$id"]
        .as_str()
        .context("generated project schema is missing $id")?
        .to_owned();
    let model_id = schemas.model["$id"]
        .as_str()
        .context("generated model schema is missing $id")?
        .to_owned();
    let project_bytes = pretty(&schemas.project)?;
    let model_bytes = pretty(&schemas.model)?;

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
        unclassified: 0,
        project_schema_bytes: project_bytes.len(),
        model_schema_bytes: model_bytes.len(),
        client_tracker: api.client_tracker.clone(),
    };

    let sources = BTreeMap::from([
        ("clientTracker".to_owned(), tracker_source),
        ("creatorDocs".to_owned(), docs_source),
        ("reflection".to_owned(), reflection_source),
        ("rojo".to_owned(), rojo.source),
    ]);
    let limitations = vec![
        "The class produced by a filesystem path cannot be selected statically, so path-backed nodes use the reflected property-name set and Rojo's general unresolved-value grammar.".to_owned(),
        "Serializer formats without a reflected default sample are represented conservatively instead of being guessed.".to_owned(),
        "Rojo runtime validation, filesystem access, and glob compilation are outside JSON Schema.".to_owned(),
    ];
    let manifest = Manifest {
        generator: format!("rojo-schema {}", env!("CARGO_PKG_VERSION")),
        schema_draft: schema::DRAFT.to_owned(),
        project_schema_id: project_id.clone(),
        model_schema_id: model_id.clone(),
        sources: sources.clone(),
        counts: stats.clone(),
        limitations,
    };

    let mut classifications = BTreeMap::new();
    for item in &api.coverage {
        let key = classification_name(item.classification);
        *classifications.entry(key).or_insert(0) += 1;
    }
    let coverage_sources = sources
        .iter()
        .map(|(name, source)| (name.clone(), source.version.clone()))
        .collect();
    let coverage = Coverage {
        sources: coverage_sources,
        counts: stats.clone(),
        classifications,
        docs: api.docs_counts,
        variant_types: api.variant_types,
        deprecation_overrides: api.deprecation_overrides,
        diagnostics: api.diagnostics,
        client_tracker: api.client_tracker,
        items: api.coverage,
    };

    let manifest_bytes = pretty(&manifest)?;
    let coverage_bytes = pretty(&coverage)?;

    Ok(Artifacts {
        project: project_bytes,
        model: model_bytes,
        manifest: manifest_bytes,
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
    write_file(&config.manifest, &artifacts.manifest)?;
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
        || first.manifest != second.manifest
        || first.coverage != second.coverage
    {
        bail!("generation is nondeterministic for identical inputs");
    }

    let expected = [
        (&config.project, &first.project, "project schema"),
        (&config.model, &first.model, "model schema"),
        (&config.manifest, &first.manifest, "manifest"),
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
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)
            .with_context(|| format!("creating output directory {}", parent.display()))?;
    }
    fs::write(path, bytes).with_context(|| format!("writing {}", path.display()))
}

fn pretty(value: &impl Serialize) -> Result<Vec<u8>> {
    let mut bytes = serde_json::to_vec_pretty(value)?;
    bytes.push(b'\n');
    Ok(bytes)
}

fn classification_name(classification: Classification) -> String {
    match classification {
        Classification::Matched => "matched",
        Classification::ApiOnly => "api-only",
        Classification::ReflectionOnly => "reflection-only",
        Classification::TypeConflict => "type-conflict",
        Classification::MetadataConflict => "metadata-conflict",
        Classification::NonProjectable => "non-projectable",
        Classification::Unsupported => "unsupported",
    }
    .to_owned()
}

/// Read source provenance without writing generated artifacts.
///
/// # Errors
///
/// Returns an error when a source checkout or its version metadata cannot be
/// read or validated.
pub fn source_versions(config: &Config) -> Result<BTreeMap<String, SourceInfo>> {
    let rojo = source::load_rojo(&config.rojo)?;
    let docs = docs::load(&config.docs)?;
    tracker::load(&config.tracker)?;
    Ok(BTreeMap::from([
        (
            "clientTracker".to_owned(),
            source::tracker_source(&config.tracker)?,
        ),
        (
            "creatorDocs".to_owned(),
            source::docs_source(&config.docs, &docs.studio_version)?,
        ),
        (
            "reflection".to_owned(),
            source::reflection_source(&config.rojo)?,
        ),
        ("rojo".to_owned(), rojo.source),
    ]))
}

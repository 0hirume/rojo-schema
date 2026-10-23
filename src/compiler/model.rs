use std::collections::BTreeMap;

use serde::Serialize;
use serde_json::Value;

#[derive(Debug, Clone)]
pub(super) struct Api {
    pub studio_version: String,
    pub reflection_version: String,
    pub classes: BTreeMap<String, Class>,
    pub enums: BTreeMap<String, Enum>,
    pub variant_types: Vec<String>,
    pub coverage: Vec<CoverageItem>,
    pub diagnostics: Vec<Diagnostic>,
    pub docs_counts: BTreeMap<String, usize>,
    pub deprecation_overrides: Vec<DeprecationOverride>,
    pub client_tracker: ClientTrackerStats,
}

#[derive(Debug, Clone)]
pub(super) struct Class {
    pub name: String,
    pub superclass: Option<String>,
    pub tags: Vec<String>,
    pub summary: Option<String>,
    pub description: Option<String>,
    pub deprecated: bool,
    pub deprecation_message: Option<String>,
    pub properties: BTreeMap<String, Property>,
}

#[derive(Debug, Clone)]
pub(super) struct Property {
    pub name: String,
    pub owner: String,
    pub data_type: PropertyType,
    pub kind: String,
    pub serialization: String,
    pub alias_for: Option<String>,
    pub migration_targets: Vec<String>,
    pub scriptability: String,
    pub tags: Vec<String>,
    pub default: Option<Value>,
    pub summary: Option<String>,
    pub description: Option<String>,
    pub deprecated: bool,
    pub deprecation_message: Option<String>,
    pub security: Option<Value>,
    pub thread_safety: Option<String>,
    pub docs_serialization: Option<Value>,
    pub capabilities: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum PropertyType {
    Enum(String),
    Value(String),
}

impl PropertyType {
    #[must_use]
    pub(super) fn name(&self) -> &str {
        match self {
            Self::Enum(name) | Self::Value(name) => name,
        }
    }
}

#[derive(Debug, Clone)]
pub(super) struct Enum {
    pub summary: Option<String>,
    pub description: Option<String>,
    pub deprecated: bool,
    pub deprecation_message: Option<String>,
    pub items: BTreeMap<String, EnumItem>,
}

#[derive(Debug, Clone)]
pub(super) struct EnumItem {
    pub name: String,
    pub value: u32,
    pub summary: Option<String>,
    pub description: Option<String>,
    pub deprecated: bool,
    pub deprecation_message: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(super) enum Classification {
    Matched,
    ApiOnly,
    ReflectionOnly,
    TypeConflict,
    MetadataConflict,
    NonProjectable,
    Unsupported,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct CoverageItem {
    pub source: String,
    pub kind: String,
    pub name: String,
    pub classification: Classification,
    pub projectable: bool,
    pub disposition: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_type: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub schema_ref: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Diagnostic {
    pub name: String,
    pub classification: Classification,
    pub reflection: String,
    pub api: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct DeprecationOverride {
    pub property: String,
    pub upstream: String,
    pub reason: String,
}

/// Client Tracker API inventory counts.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientTrackerStats {
    /// Classes in the tracker inventory.
    pub classes: usize,

    /// Properties in the tracker inventory.
    pub properties: usize,

    /// Methods in the tracker inventory.
    pub methods: usize,

    /// Events in the tracker inventory.
    pub events: usize,

    /// Callbacks in the tracker inventory.
    pub callbacks: usize,

    /// Members outside the recognized categories.
    pub other_members: usize,

    /// Enumerations in the tracker inventory.
    pub enums: usize,

    /// Enumeration items in the tracker inventory.
    pub enum_items: usize,
}

/// Schema size, coverage, and source-conflict statistics.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Stats {
    /// Reflected classes.
    pub classes: usize,

    /// Properties before inheritance expansion.
    pub properties: usize,

    /// Properties after inheritance expansion.
    pub flattened_properties: usize,

    /// Reflected enumerations.
    pub enums: usize,

    /// Reflected enumeration items.
    pub enum_items: usize,

    /// Definitions in the project schema.
    pub project_definitions: usize,

    /// Definitions in the model schema.
    pub model_definitions: usize,

    /// Property value variants encountered.
    pub variant_types: usize,

    /// Entries in the API coverage report.
    pub api_items: usize,

    /// Disagreements among input sources.
    pub conflicts: usize,

    /// Input inventory entries missing from coverage.
    pub unclassified: usize,

    /// Serialized project schema size in bytes.
    pub project_schema_bytes: usize,

    /// Serialized model schema size in bytes.
    pub model_schema_bytes: usize,

    /// Client Tracker inventory counts.
    pub client_tracker: ClientTrackerStats,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Coverage {
    pub counts: Stats,
    pub classifications: BTreeMap<Classification, usize>,
    pub docs: BTreeMap<String, usize>,
    pub variant_types: Vec<String>,
    pub deprecation_overrides: Vec<DeprecationOverride>,
    pub diagnostics: Vec<Diagnostic>,
    pub client_tracker: ClientTrackerStats,
    pub items: Vec<CoverageItem>,
}

use std::collections::BTreeMap;

use serde::Serialize;
use serde_json::Value;

#[derive(Debug, Clone)]
pub struct Api {
    pub studio_version: String,
    pub reflection_version: String,
    pub classes: BTreeMap<String, Class>,
    pub enums: BTreeMap<String, Enum>,
    pub variant_types: Vec<String>,
    pub coverage: Vec<CoverageItem>,
    pub diagnostics: Vec<Diagnostic>,
    pub docs_counts: BTreeMap<String, usize>,
}

#[derive(Debug, Clone)]
pub struct Class {
    pub name: String,
    pub superclass: Option<String>,
    pub tags: Vec<String>,
    pub summary: Option<String>,
    pub description: Option<String>,
    pub deprecated: bool,
    pub deprecation_message: Option<String>,
    pub properties: BTreeMap<String, Property>,
}

impl Class {
    #[must_use]
    pub fn is_service(&self) -> bool {
        self.tags.iter().any(|tag| tag == "Service")
    }
}

#[derive(Debug, Clone)]
pub struct Property {
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
    pub capabilities: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PropertyType {
    Enum(String),
    Value(String),
}

impl PropertyType {
    #[must_use]
    pub fn name(&self) -> &str {
        match self {
            Self::Enum(name) | Self::Value(name) => name,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Enum {
    pub name: String,
    pub summary: Option<String>,
    pub description: Option<String>,
    pub deprecated: bool,
    pub deprecation_message: Option<String>,
    pub items: BTreeMap<String, EnumItem>,
}

#[derive(Debug, Clone)]
pub struct EnumItem {
    pub name: String,
    pub value: u32,
    pub summary: Option<String>,
    pub deprecated: bool,
    pub deprecation_message: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Classification {
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
pub struct CoverageItem {
    pub source: String,
    pub kind: String,
    pub name: String,
    pub classification: Classification,
    pub projectable: bool,
    pub disposition: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub schema_ref: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Diagnostic {
    pub name: String,
    pub classification: Classification,
    pub reflection: String,
    pub api: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceInfo {
    pub repository: String,
    pub version: String,
    pub sha256: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revision: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Stats {
    pub classes: usize,
    pub properties: usize,
    pub flattened_properties: usize,
    pub enums: usize,
    pub enum_items: usize,
    pub definitions: usize,
    pub variant_types: usize,
    pub api_items: usize,
    pub conflicts: usize,
    pub unclassified: usize,
    pub schema_bytes: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub generator: String,
    pub schema_draft: String,
    pub schema_id: String,
    pub sources: BTreeMap<String, SourceInfo>,
    pub counts: Stats,
    pub limitations: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Coverage {
    pub sources: BTreeMap<String, String>,
    pub counts: Stats,
    pub classifications: BTreeMap<String, usize>,
    pub docs: BTreeMap<String, usize>,
    pub variant_types: Vec<String>,
    pub diagnostics: Vec<Diagnostic>,
    pub items: Vec<CoverageItem>,
}

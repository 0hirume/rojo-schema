use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use anyhow::{bail, Context, Result};
use serde::Deserialize;
use serde_json::Value;

use crate::model::DeprecationOverride;

#[derive(Debug, Clone, Default)]
pub struct Catalog {
    pub studio_version: String,
    pub classes: BTreeMap<String, Doc>,
    pub enums: BTreeMap<String, Doc>,
    pub datatypes: BTreeMap<String, Doc>,
    pub globals: BTreeMap<String, Doc>,
    pub libraries: BTreeMap<String, Doc>,
    pub deprecation_overrides: Vec<DeprecationOverride>,
}

#[derive(Debug, Clone, Default)]
pub struct Doc {
    pub name: String,
    pub kind: String,
    pub summary: Option<String>,
    pub description: Option<String>,
    pub tags: Vec<String>,
    pub deprecation_message: Option<String>,
    pub inherits: Vec<String>,
    pub members: BTreeMap<String, Vec<Member>>,
}

impl Doc {
    pub fn deprecated(&self) -> bool {
        self.tags.iter().any(|tag| tag == "Deprecated")
            || self
                .deprecation_message
                .as_deref()
                .is_some_and(|text| !text.is_empty())
    }

    pub fn member(&self, kind: &str, short_name: &str) -> Option<&Member> {
        self.members.get(kind)?.iter().find(|member| {
            member.name == short_name
                || member
                    .name
                    .rsplit_once('.')
                    .is_some_and(|(_, name)| name == short_name)
        })
    }
}

#[derive(Debug, Clone, Default)]
pub struct Member {
    pub name: String,
    pub data_type: Option<String>,
    pub value: Option<u32>,
    pub summary: Option<String>,
    pub description: Option<String>,
    pub tags: Vec<String>,
    pub deprecation_message: Option<String>,
    pub security: Option<Value>,
    pub thread_safety: Option<String>,
    pub serialization: Option<Value>,
    pub capabilities: Vec<String>,
    pub deprecation_warning_suppressed: bool,
}

impl Member {
    pub fn deprecated(&self) -> bool {
        !self.deprecation_warning_suppressed
            && (self.tags.iter().any(|tag| tag == "Deprecated")
                || self
                    .deprecation_message
                    .as_deref()
                    .is_some_and(|text| !text.is_empty()))
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Deprecations {
    properties: BTreeMap<String, Deprecation>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Deprecation {
    reason: String,
}

pub fn engine_root(path: &Path) -> Result<PathBuf> {
    let direct = path.join("classes");
    if direct.is_dir() && path.join("STUDIO_VERSION").is_file() {
        return Ok(path.to_path_buf());
    }

    let nested = path.join("content/en-us/reference/engine");
    if nested.join("classes").is_dir() && nested.join("STUDIO_VERSION").is_file() {
        return Ok(nested);
    }

    bail!(
        "Creator Docs engine reference not found beneath {}",
        path.display()
    )
}

pub fn load(path: &Path) -> Result<Catalog> {
    let root = engine_root(path)?;
    let studio_version = fs::read_to_string(root.join("STUDIO_VERSION"))
        .with_context(|| format!("reading {}/STUDIO_VERSION", root.display()))?
        .lines()
        .next()
        .unwrap_or_default()
        .trim()
        .to_owned();

    let mut catalog = Catalog {
        studio_version,
        ..Catalog::default()
    };

    catalog.classes = load_dir(&root.join("classes"))?;
    catalog.enums = load_dir(&root.join("enums"))?;
    catalog.datatypes = load_dir(&root.join("datatypes"))?;
    catalog.globals = load_dir(&root.join("globals"))?;
    catalog.libraries = load_dir(&root.join("libraries"))?;
    let deprecations = toml::from_str(include_str!("../deprecations.toml"))
        .context("parsing embedded deprecations.toml")?;
    catalog.deprecation_overrides = apply_deprecations(&mut catalog, &deprecations)?;

    Ok(catalog)
}

fn apply_deprecations(
    catalog: &mut Catalog,
    deprecations: &Deprecations,
) -> Result<Vec<DeprecationOverride>> {
    let mut applied = Vec::with_capacity(deprecations.properties.len());
    for (property_name, deprecation) in &deprecations.properties {
        let (class_name, _) = property_name
            .split_once('.')
            .with_context(|| format!("invalid deprecation property name: {property_name}"))?;
        let class = catalog
            .classes
            .get_mut(class_name)
            .with_context(|| format!("deprecation property class not found: {property_name}"))?;
        let property = class
            .members
            .get_mut("properties")
            .and_then(|properties| {
                properties
                    .iter_mut()
                    .find(|property| property.name == *property_name)
            })
            .with_context(|| format!("deprecation property not found: {property_name}"))?;
        let upstream = property
            .deprecation_message
            .take()
            .with_context(|| format!("deprecation message missing: {property_name}"))?;
        let reason = deprecation.reason.trim();
        if reason.is_empty() {
            bail!("deprecation reason is empty: {property_name}");
        }

        let note = format!("Compatibility note: {reason}\n\nUpstream deprecation note: {upstream}");
        property.description = Some(match property.description.take() {
            Some(description) => format!("{description}\n\n{note}"),
            None => note,
        });
        property.deprecation_warning_suppressed = true;
        applied.push(DeprecationOverride {
            property: property_name.clone(),
            upstream,
            reason: reason.to_owned(),
        });
    }
    Ok(applied)
}

fn load_dir(path: &Path) -> Result<BTreeMap<String, Doc>> {
    let mut paths = fs::read_dir(path)
        .with_context(|| format!("reading docs directory {}", path.display()))?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<std::io::Result<Vec<_>>>()?;
    paths.retain(|path| {
        path.extension()
            .is_some_and(|extension| extension == "yaml")
    });
    paths.sort();

    let mut docs = BTreeMap::new();
    for path in paths {
        let text = fs::read_to_string(&path)
            .with_context(|| format!("reading docs file {}", path.display()))?;
        let value: Value = serde_yaml::from_str(&text)
            .with_context(|| format!("parsing docs file {}", path.display()))?;
        let doc = parse_doc(&value)
            .with_context(|| format!("normalizing docs file {}", path.display()))?;
        docs.insert(doc.name.clone(), doc);
    }
    Ok(docs)
}

fn parse_doc(value: &Value) -> Result<Doc> {
    let name = string(value, "name").context("missing name")?;
    let kind = string(value, "type").context("missing type")?;
    let mut members = BTreeMap::new();
    for member_kind in [
        "properties",
        "methods",
        "events",
        "callbacks",
        "items",
        "constructors",
        "constants",
        "functions",
    ] {
        let parsed = value
            .get(member_kind)
            .and_then(Value::as_array)
            .map(|items| items.iter().map(parse_member).collect::<Vec<_>>())
            .unwrap_or_default();
        members.insert(member_kind.to_owned(), parsed);
    }

    Ok(Doc {
        name,
        kind,
        summary: text(value, "summary"),
        description: text(value, "description"),
        tags: strings(value, "tags"),
        deprecation_message: text(value, "deprecation_message"),
        inherits: strings(value, "inherits"),
        members,
    })
}

fn parse_member(value: &Value) -> Member {
    Member {
        name: string(value, "name").unwrap_or_else(|| "<unnamed>".to_owned()),
        data_type: string(value, "type"),
        value: value
            .get("value")
            .and_then(Value::as_u64)
            .and_then(|value| u32::try_from(value).ok()),
        summary: text(value, "summary"),
        description: text(value, "description"),
        tags: strings(value, "tags"),
        deprecation_message: text(value, "deprecation_message"),
        security: value.get("security").cloned().filter(not_null),
        thread_safety: string(value, "thread_safety"),
        serialization: value.get("serialization").cloned().filter(not_null),
        capabilities: strings(value, "capabilities"),
        deprecation_warning_suppressed: false,
    }
}

fn not_null(value: &Value) -> bool {
    !value.is_null()
}

fn string(value: &Value, key: &str) -> Option<String> {
    value.get(key)?.as_str().map(ToOwned::to_owned)
}

fn text(value: &Value, key: &str) -> Option<String> {
    string(value, key)
        .map(|text| text.trim().to_owned())
        .filter(|text| !text.is_empty())
}

fn strings(value: &Value, key: &str) -> Vec<String> {
    value
        .get(key)
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .map(ToOwned::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn catalog(message: &str) -> Catalog {
        let mut class = Doc {
            name: "Lighting".to_owned(),
            ..Doc::default()
        };
        class.members.insert(
            "properties".to_owned(),
            vec![Member {
                name: "Lighting.Technology".to_owned(),
                description: Some("Lighting mode.".to_owned()),
                deprecation_message: Some(message.to_owned()),
                ..Member::default()
            }],
        );
        Catalog {
            classes: BTreeMap::from([("Lighting".to_owned(), class)]),
            ..Catalog::default()
        }
    }

    fn deprecations() -> Deprecations {
        Deprecations {
            properties: BTreeMap::from([(
                "Lighting.Technology".to_owned(),
                Deprecation {
                    reason: "Required for compatibility.".to_owned(),
                },
            )]),
        }
    }

    #[test]
    fn applies_deprecation_override() {
        let mut catalog = catalog("Superseded by newer lighting controls.");
        let applied = apply_deprecations(&mut catalog, &deprecations()).unwrap();
        let property = catalog.classes["Lighting"]
            .member("properties", "Technology")
            .unwrap();

        assert!(!property.deprecated());
        assert!(property.deprecation_message.is_none());
        assert!(property
            .description
            .as_deref()
            .unwrap()
            .contains("Superseded by newer lighting controls."));
        assert_eq!(applied[0].property, "Lighting.Technology");
    }

    #[test]
    fn rejects_missing_deprecation_message() {
        let mut catalog = catalog("Superseded.");
        catalog
            .classes
            .get_mut("Lighting")
            .unwrap()
            .members
            .get_mut("properties")
            .unwrap()[0]
            .deprecation_message = None;
        let error = apply_deprecations(&mut catalog, &deprecations())
            .unwrap_err()
            .to_string();

        assert!(error.contains("deprecation message missing"));
    }
}

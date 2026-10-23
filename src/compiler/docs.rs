use std::{
    collections::BTreeMap,
    fs, io,
    path::{Path, PathBuf},
    result,
};

use eyre::{Context as _, ContextCompat as _, Result, bail, ensure};
use serde::{Deserialize, Deserializer};
use serde_json::Value;
use url::Url;

use super::{model::DeprecationOverride, presentation};

#[derive(Debug, Clone, Default)]
pub(super) struct Catalog {
    pub studio_version: String,
    pub classes: BTreeMap<String, Doc>,
    pub enums: BTreeMap<String, Doc>,
    pub datatypes: BTreeMap<String, Doc>,
    pub globals: BTreeMap<String, Doc>,
    pub libraries: BTreeMap<String, Doc>,
    pub deprecation_overrides: Vec<DeprecationOverride>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub(super) struct Doc {
    pub name: String,

    #[serde(rename = "type")]
    pub kind: String,

    #[serde(default, deserialize_with = "text")]
    pub summary: Option<String>,

    #[serde(default, deserialize_with = "text")]
    pub description: Option<String>,

    #[serde(default, deserialize_with = "strings")]
    pub tags: Vec<String>,

    #[serde(default, deserialize_with = "text")]
    pub deprecation_message: Option<String>,

    #[serde(default, deserialize_with = "strings")]
    pub inherits: Vec<String>,

    #[serde(skip)]
    pub members: BTreeMap<String, Vec<Member>>,
}

impl Doc {
    pub(super) fn deprecated(&self) -> bool {
        self.tags.iter().any(|tag| tag == "Deprecated")
            || self
                .deprecation_message
                .as_deref()
                .is_some_and(|text| !text.is_empty())
    }

    pub(super) fn member(&self, kind: &str, short_name: &str) -> Option<&Member> {
        self.members.get(kind)?.iter().find(|member| {
            member.name == short_name
                || member
                    .name
                    .rsplit_once('.')
                    .is_some_and(|(_, name)| name == short_name)
        })
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub(super) struct Member {
    pub name: String,

    #[serde(rename = "type")]
    pub data_type: Option<String>,

    pub value: Option<u32>,

    #[serde(default, deserialize_with = "text")]
    pub summary: Option<String>,

    #[serde(default, deserialize_with = "text")]
    pub description: Option<String>,

    #[serde(default, deserialize_with = "strings")]
    pub tags: Vec<String>,

    #[serde(default, deserialize_with = "text")]
    pub deprecation_message: Option<String>,

    pub security: Option<Value>,
    pub thread_safety: Option<String>,
    pub serialization: Option<Value>,

    #[serde(default, deserialize_with = "strings")]
    pub capabilities: Vec<String>,

    #[serde(skip)]
    pub deprecation_warning_suppressed: bool,
}

impl Member {
    pub(super) fn deprecated(&self) -> bool {
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

#[derive(Deserialize)]
struct Document {
    #[serde(flatten)]
    doc: Doc,

    properties: Option<Vec<Member>>,
    methods: Option<Vec<Member>>,
    events: Option<Vec<Member>>,
    callbacks: Option<Vec<Member>>,
    items: Option<Vec<Member>>,
    constructors: Option<Vec<Member>>,
    constants: Option<Vec<Member>>,
    functions: Option<Vec<Member>>,
}

pub(super) fn engine_root(path: &Path) -> Result<PathBuf> {
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

pub(super) fn load(path: &Path) -> Result<Catalog> {
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

    let deprecations = toml::from_str(include_str!("../../deprecations.toml"))
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
        .collect::<io::Result<Vec<_>>>()?;

    paths.retain(|path| {
        path.extension()
            .is_some_and(|extension| extension == "yaml")
    });

    paths.sort();

    let mut docs = BTreeMap::new();

    for path in paths {
        let text = fs::read_to_string(&path)
            .with_context(|| format!("reading docs file {}", path.display()))?;

        let document: Document =
            serde_path_to_error::deserialize(yaml_serde::Deserializer::from_str(&text))
                .with_context(|| format!("parsing docs file {}", path.display()))?;

        let mut doc = document.doc;

        ensure!(
            !doc.name.trim().is_empty(),
            "empty document name in {}",
            path.display()
        );

        doc.members = [
            ("properties", document.properties),
            ("methods", document.methods),
            ("events", document.events),
            ("callbacks", document.callbacks),
            ("items", document.items),
            ("constructors", document.constructors),
            ("constants", document.constants),
            ("functions", document.functions),
        ]
        .into_iter()
        .map(|(kind, members)| (kind.to_owned(), members.unwrap_or_default()))
        .collect();

        for member in doc.members.values().flatten() {
            ensure!(
                !member.name.trim().is_empty(),
                "empty member name in {}",
                path.display()
            );
        }

        let name = doc.name.clone();

        let directory = path
            .parent()
            .and_then(Path::file_name)
            .and_then(|name| name.to_str())
            .context("documentation directory has no name")?;

        let base = Url::parse(&format!(
            "https://create.roblox.com/docs/reference/engine/{directory}/{name}"
        ))
        .context("building documentation URL")?;

        for text in [
            &mut doc.summary,
            &mut doc.description,
            &mut doc.deprecation_message,
        ] {
            *text = text
                .as_deref()
                .map(|text| presentation::markdown(text, &base))
                .transpose()?;
        }

        for member in doc.members.values_mut().flatten() {
            let mut base = base.clone();

            let name = member
                .name
                .rsplit_once('.')
                .map_or(member.name.as_str(), |(_, name)| name);

            base.set_fragment(Some(name.trim_end_matches("()")));

            for text in [
                &mut member.summary,
                &mut member.description,
                &mut member.deprecation_message,
            ] {
                *text = text
                    .as_deref()
                    .map(|text| presentation::markdown(text, &base))
                    .transpose()?;
            }
        }

        ensure!(
            docs.insert(name.clone(), doc).is_none(),
            "duplicate documentation name: {name}"
        );
    }

    Ok(docs)
}

fn text<'de, D: Deserializer<'de>>(deserializer: D) -> result::Result<Option<String>, D::Error> {
    Ok(Option::<String>::deserialize(deserializer)?
        .map(|text| text.trim().to_owned())
        .filter(|text| !text.is_empty()))
}

fn strings<'de, D: Deserializer<'de>>(deserializer: D) -> result::Result<Vec<String>, D::Error> {
    Ok(Option::<Vec<String>>::deserialize(deserializer)?.unwrap_or_default())
}

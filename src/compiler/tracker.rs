use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use eyre::{Context as _, ContextCompat as _, Result, bail, ensure};
use serde::Deserialize;
use serde_json::Value;

const DUMP_FILE: &str = "Full-API-Dump.json";
const VERSION_FILE: &str = "version.txt";

#[derive(Debug, Clone)]
pub(super) struct Catalog {
    pub classes: BTreeMap<String, Class>,
    pub enums: BTreeMap<String, Enum>,
}

#[derive(Debug, Clone)]
pub(super) struct Class {
    pub members: BTreeMap<String, Member>,
}

#[derive(Debug, Clone)]
pub(super) struct Member {
    pub name: String,
    pub kind: String,
    pub type_category: Option<String>,
    pub type_name: Option<String>,
    pub security: Option<Value>,
    pub serialization: Option<Value>,
    pub thread_safety: Option<String>,
    pub tags: Vec<Value>,
    pub capabilities: Option<Value>,
}

#[derive(Debug, Clone)]
pub(super) struct Enum {
    pub items: BTreeMap<String, u32>,
}

#[derive(Debug, Deserialize)]
struct Dump {
    #[serde(rename = "Version")]
    _version: u32,

    #[serde(rename = "Classes")]
    classes: Vec<RawClass>,

    #[serde(rename = "Enums")]
    enums: Vec<RawEnum>,
}

#[derive(Debug, Deserialize)]
struct RawClass {
    #[serde(rename = "Name")]
    name: String,

    #[serde(rename = "Members", default)]
    members: Vec<RawMember>,
}

#[derive(Debug, Deserialize)]
struct RawMember {
    #[serde(rename = "MemberType")]
    kind: String,

    #[serde(rename = "Name")]
    name: String,

    #[serde(rename = "ValueType")]
    value_type: Option<RawType>,

    #[serde(rename = "Security")]
    security: Option<Value>,

    #[serde(rename = "Serialization")]
    serialization: Option<Value>,

    #[serde(rename = "ThreadSafety")]
    thread_safety: Option<String>,

    #[serde(rename = "Tags", default)]
    tags: Vec<Value>,

    #[serde(rename = "Capabilities")]
    capabilities: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct RawType {
    #[serde(rename = "Category")]
    category: Option<String>,

    #[serde(rename = "Name")]
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawEnum {
    #[serde(rename = "Name")]
    name: String,

    #[serde(rename = "Items", default)]
    items: Vec<RawEnumItem>,
}

#[derive(Debug, Deserialize)]
struct RawEnumItem {
    #[serde(rename = "Name")]
    name: String,

    #[serde(rename = "Value")]
    value: u32,
}

pub(super) fn load(path: &Path) -> Result<Catalog> {
    let root = root(path)?;
    let dump_path = root.join(DUMP_FILE);
    let version_path = root.join(VERSION_FILE);

    let dump = fs::read_to_string(&dump_path)
        .with_context(|| format!("reading {}", dump_path.display()))?;

    let version = fs::read_to_string(&version_path)
        .with_context(|| format!("reading {}", version_path.display()))?
        .lines()
        .next()
        .unwrap_or_default()
        .trim()
        .to_owned();

    ensure!(
        !version.is_empty(),
        "{} has no Studio version",
        version_path.display()
    );

    let mut decoder = serde_json::Deserializer::from_str(&dump);

    let dump: Dump = serde_path_to_error::deserialize(&mut decoder)
        .with_context(|| format!("parsing {}", dump_path.display()))?;

    decoder
        .end()
        .with_context(|| format!("parsing {}", dump_path.display()))?;

    parse_dump(dump)
}

pub(super) fn root(path: &Path) -> Result<PathBuf> {
    let root = if path.is_file() {
        path.parent()
            .context("Client Tracker dump has no parent directory")?
    } else {
        path
    };

    ensure!(
        root.is_dir(),
        "Client Tracker source not found at {}",
        root.display()
    );

    for file in [DUMP_FILE, VERSION_FILE] {
        ensure!(
            root.join(file).is_file(),
            "Client Tracker file not found at {}",
            root.join(file).display()
        );
    }

    Ok(root.to_owned())
}

fn parse_dump(dump: Dump) -> Result<Catalog> {
    let mut classes = BTreeMap::new();

    for raw in dump.classes {
        let name = required_name(&raw.name, "class")?;
        let mut members = BTreeMap::new();
        let mut raw_member_names = BTreeMap::new();

        for raw_member in raw.members {
            let member_name = required_name(&raw_member.name, "class member")?;

            let member = Member {
                name: member_name.clone(),
                kind: raw_member.kind,
                type_category: raw_member
                    .value_type
                    .as_ref()
                    .and_then(|value| value.category.clone()),
                type_name: raw_member.value_type.and_then(|value| value.name),
                security: raw_member.security,
                serialization: raw_member.serialization,
                thread_safety: raw_member.thread_safety,
                tags: raw_member.tags,
                capabilities: raw_member.capabilities,
            };

            if let Some(existing) = raw_member_names.get(&member_name) {
                let current_is_canonical = raw_member.name == member_name;
                let existing_is_canonical = existing == &member_name;

                if current_is_canonical && !existing_is_canonical {
                    members.insert(member_name.clone(), member);
                    raw_member_names.insert(member_name, raw_member.name);
                    continue;
                }

                if !current_is_canonical && existing_is_canonical {
                    continue;
                }

                bail!("duplicate Client Tracker member: {name}.{member_name}");
            }

            raw_member_names.insert(member_name.clone(), raw_member.name);
            members.insert(member_name, member);
        }

        if classes.insert(name.clone(), Class { members }).is_some() {
            bail!("duplicate Client Tracker class: {name}");
        }
    }

    let mut enums = BTreeMap::new();

    for raw in dump.enums {
        let name = required_name(&raw.name, "enum")?;
        let mut items = BTreeMap::new();

        for item in raw.items {
            let item_name = required_name(&item.name, "enum item")?;

            if items.insert(item_name.clone(), item.value).is_some() {
                bail!("duplicate Client Tracker enum item: {name}.{item_name}");
            }
        }

        if enums.insert(name.clone(), Enum { items }).is_some() {
            bail!("duplicate Client Tracker enum: {name}");
        }
    }

    Ok(Catalog { classes, enums })
}

fn required_name(name: &str, kind: &str) -> Result<String> {
    let name = name.trim();
    ensure!(!name.is_empty(), "Client Tracker {kind} has an empty name");

    Ok(name.to_owned())
}

use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use anyhow::{bail, ensure, Context, Result};
use serde::Deserialize;

const DUMP_FILE: &str = "Full-API-Dump.json";
const VERSION_FILE: &str = "version.txt";

#[derive(Debug, Clone)]
pub struct Catalog {
    pub classes: BTreeMap<String, Class>,
    pub enums: BTreeMap<String, Enum>,
}

#[derive(Debug, Clone)]
pub struct Class {
    pub members: BTreeMap<String, Member>,
}

#[derive(Debug, Clone)]
pub struct Member {
    pub name: String,
    pub kind: String,
    pub type_category: Option<String>,
    pub type_name: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Enum {
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

pub fn load(path: &Path) -> Result<Catalog> {
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

    let dump: Dump =
        serde_json::from_str(&dump).with_context(|| format!("parsing {}", dump_path.display()))?;
    parse_dump(dump)
}

pub fn root(path: &Path) -> Result<PathBuf> {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn dump() -> Dump {
        serde_json::from_str(
            r#"
            {
              "Version": 1,
              "Classes": [{
                "Name": "Part",
                "Members": [
                  {"MemberType": "Property", "Name": "Anchored", "ValueType": {"Category": "Primitive", "Name": "bool"}},
                  {"MemberType": "Function", "Name": "Clone"}
                ]
              }],
              "Enums": [{
                "Name": "Material",
                "Items": [{"Name": "Plastic", "Value": 256}, {"Name": "Wood", "Value": 512}]
              }]
            }
            "#,
        )
        .unwrap()
    }

    #[test]
    fn parses_inventory() {
        let catalog = parse_dump(dump()).unwrap();

        assert_eq!(catalog.classes["Part"].members["Anchored"].kind, "Property");
        assert_eq!(
            catalog.classes["Part"].members["Anchored"].type_category,
            Some("Primitive".to_owned())
        );
        assert_eq!(
            catalog.classes["Part"].members["Anchored"].type_name,
            Some("bool".to_owned())
        );
        assert_eq!(catalog.enums["Material"].items["Wood"], 512);
    }

    #[test]
    fn ignores_whitespace_only_duplicate_members() {
        let mut dump = dump();
        dump.classes[0].members.push(RawMember {
            kind: "Property".to_owned(),
            name: "Clone ".to_owned(),
            value_type: None,
        });
        let catalog = parse_dump(dump).unwrap();
        assert_eq!(catalog.classes["Part"].members.len(), 2);
        assert_eq!(catalog.classes["Part"].members["Clone"].kind, "Function");
    }

    #[test]
    fn rejects_duplicate_names() {
        let mut dump = dump();
        dump.classes.push(RawClass {
            name: "Part".to_owned(),
            members: Vec::new(),
        });
        let error = parse_dump(dump).unwrap_err().to_string();
        assert!(error.contains("duplicate Client Tracker class"));
    }
}

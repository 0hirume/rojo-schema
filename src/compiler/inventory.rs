use std::collections::BTreeSet;

use rbx_reflection::ReflectionDatabase;

use super::{docs, model::CoverageItem, tracker};

pub(super) fn unclassified(
    docs: &docs::Catalog,
    tracker: &tracker::Catalog,
    database: &ReflectionDatabase<'_>,
    variants: &[String],
    coverage: &[CoverageItem],
) -> usize {
    let mut classified = BTreeSet::new();

    for item in coverage {
        for source in item.source.split('+') {
            classified.insert((source.to_owned(), item.kind.clone(), item.name.clone()));
        }
    }

    let mut missing = BTreeSet::new();

    let mut account = |source: &str, kind: &str, name: &str| {
        let key = (source.to_owned(), kind.to_owned(), name.to_owned());

        if !classified.contains(&key) {
            missing.insert(key);
        }
    };

    for (name, class) in &database.classes {
        account("reflection", "class", name);

        for property in class.properties.keys() {
            account("reflection", "property", &format!("{name}.{property}"));
        }
    }

    for (name, enumeration) in &database.enums {
        account("reflection", "enum", name);

        for item in enumeration.items.keys() {
            account("reflection", "enum-item", &format!("{name}.{item}"));
        }
    }

    for variant in variants {
        account("reflection", "variant-type", variant);
    }

    for (kind, catalog) in [
        ("class", &docs.classes),
        ("enum", &docs.enums),
        ("datatype", &docs.datatypes),
        ("global", &docs.globals),
        ("library", &docs.libraries),
    ] {
        for (name, doc) in catalog {
            account("api", kind, name);

            for (group, members) in &doc.members {
                let member_kind = match group.as_str() {
                    "properties" => "property",
                    "items" => "item",
                    group => group.strip_suffix('s').unwrap_or(group),
                };

                let member_kind = if kind == "class" {
                    member_kind.to_owned()
                } else {
                    format!("{kind}-{member_kind}")
                };

                for member in members {
                    let member_name = if kind == "enum"
                        || (kind == "class" && group == "properties" && !member.name.contains('.'))
                    {
                        format!("{name}.{}", member.name)
                    } else {
                        member.name.clone()
                    };

                    account("api", &member_kind, &member_name);
                }
            }
        }
    }

    for (name, class) in &tracker.classes {
        account("clientTracker", "class", name);

        for member in class.members.values() {
            let kind = match member.kind.as_str() {
                "Property" => "property",
                "Function" => "method",
                "Event" => "event",
                "Callback" => "callback",
                _ => "other-member",
            };

            account("clientTracker", kind, &format!("{name}.{}", member.name));
        }
    }

    for (name, enumeration) in &tracker.enums {
        account("clientTracker", "enum", name);

        for item in enumeration.items.keys() {
            account("clientTracker", "enum-item", &format!("{name}.{item}"));
        }
    }

    missing.len()
}

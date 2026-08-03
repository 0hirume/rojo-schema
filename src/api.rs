use std::collections::{BTreeMap, BTreeSet};

use rbx_reflection::{
    ClassDescriptor, DataType, EnumDescriptor, PropertyKind, PropertySerialization,
    ReflectionDatabase,
};

use crate::{
    docs::{Catalog, Doc, Member},
    model::{
        Api, Class, Classification, CoverageItem, Diagnostic, Enum, EnumItem, Property,
        PropertyType,
    },
};

pub fn build(docs: &Catalog, variants: &BTreeSet<String>) -> Api {
    Builder::new(docs, variants).build()
}

struct Builder<'a> {
    docs: &'a Catalog,
    database: &'static ReflectionDatabase<'static>,
    supported: BTreeSet<String>,
    classes: BTreeMap<String, Class>,
    enums: BTreeMap<String, Enum>,
    variants: BTreeSet<String>,
    coverage: Vec<CoverageItem>,
    diagnostics: Vec<Diagnostic>,
}

impl<'a> Builder<'a> {
    fn new(docs: &'a Catalog, variants: &BTreeSet<String>) -> Self {
        let database = rbx_reflection_database::get_bundled();
        Self {
            docs,
            database,
            supported: variants.clone(),
            classes: BTreeMap::new(),
            enums: BTreeMap::new(),
            variants: variants.clone(),
            coverage: Vec::new(),
            diagnostics: Vec::new(),
        }
    }

    fn build(mut self) -> Api {
        self.add_classes();
        add_docs_class_coverage(self.docs, &self.classes, &mut self.coverage);
        self.add_enums();
        add_docs_enum_coverage(self.docs, &self.enums, &mut self.coverage);
        add_non_projectable_docs(self.docs, &mut self.coverage);
        self.add_variant_coverage();
        self.finish()
    }

    fn add_classes(&mut self) {
        let mut classes = self.database.classes.iter().collect::<Vec<_>>();
        classes.sort_by_key(|(name, _)| **name);
        for (name, descriptor) in classes {
            self.add_class(name, descriptor);
        }
    }

    fn add_class(&mut self, name: &str, descriptor: &ClassDescriptor<'_>) {
        let doc = self.docs.classes.get(name);
        let class_classification = match doc {
            Some(doc)
                if doc.kind != "class"
                    || doc.inherits.first().map(String::as_str) != descriptor.superclass =>
            {
                self.diagnostics.push(Diagnostic {
                    name: name.to_owned(),
                    classification: Classification::MetadataConflict,
                    reflection: descriptor.superclass.unwrap_or("<root>").to_owned(),
                    api: format!(
                        "kind={}, superclass={}",
                        doc.kind,
                        doc.inherits.first().map_or("<root>", String::as_str)
                    ),
                });
                Classification::MetadataConflict
            }
            Some(_) => Classification::Matched,
            None => Classification::ReflectionOnly,
        };
        self.coverage.push(item(
            "reflection+api",
            "class",
            name,
            class_classification,
            true,
            "class-discriminated node branch",
            Some(definition_ref(&["node", name])),
        ));

        let properties = self.properties(name, descriptor, doc);
        let mut tags = descriptor
            .tags
            .iter()
            .map(|tag| format!("{tag:?}"))
            .collect::<Vec<_>>();
        tags.sort();
        self.classes.insert(
            name.to_owned(),
            Class {
                name: name.to_owned(),
                superclass: descriptor.superclass.map(ToOwned::to_owned),
                tags,
                summary: doc.and_then(|doc| doc.summary.clone()),
                description: doc.and_then(|doc| doc.description.clone()),
                deprecated: doc.is_some_and(Doc::deprecated),
                deprecation_message: doc.and_then(|doc| doc.deprecation_message.clone()),
                properties,
            },
        );
    }

    fn properties(
        &mut self,
        class_name: &str,
        descriptor: &ClassDescriptor<'_>,
        doc: Option<&Doc>,
    ) -> BTreeMap<String, Property> {
        let mut properties = BTreeMap::new();
        let mut descriptors = descriptor.properties.iter().collect::<Vec<_>>();
        descriptors.sort_by_key(|(name, _)| **name);
        for (property_name, property_descriptor) in descriptors {
            let doc_property = doc.and_then(|doc| doc.member("properties", property_name));
            let (kind, serialization, alias_for, migration_targets) =
                property_kind(&property_descriptor.kind);
            let data_type = match &property_descriptor.data_type {
                DataType::Enum(name) => PropertyType::Enum((*name).to_owned()),
                DataType::Value(variant) => {
                    let name = format!("{variant:?}");
                    self.variants.insert(name.clone());
                    PropertyType::Value(name)
                }
                _ => PropertyType::Value("<unknown>".to_owned()),
            };
            let supported_type = match &data_type {
                PropertyType::Enum(enum_name) => {
                    self.database.enums.contains_key(enum_name.as_str())
                }
                PropertyType::Value(variant) => self.supported.contains(variant.as_str()),
            };

            let (classification, reason) = classify_property(
                &PropertyCheck {
                    class_name,
                    property_name,
                    data_type: &data_type,
                    serialization: &serialization,
                    doc: doc_property,
                    supported: supported_type,
                },
                &mut self.diagnostics,
            );
            let schema_ref = if supported_type {
                Some(definition_ref(&["property", class_name, property_name]))
            } else {
                None
            };
            self.coverage.push(item(
                "reflection+api",
                "property",
                &format!("{class_name}.{property_name}"),
                classification,
                supported_type,
                &reason,
                schema_ref,
            ));

            let mut tags = property_descriptor
                .tags
                .iter()
                .map(|tag| format!("{tag:?}"))
                .collect::<Vec<_>>();
            tags.sort();
            properties.insert(
                (*property_name).to_owned(),
                Property {
                    name: (*property_name).to_owned(),
                    owner: class_name.to_owned(),
                    data_type,
                    kind,
                    serialization,
                    alias_for,
                    migration_targets,
                    scriptability: format!("{:?}", property_descriptor.scriptability),
                    tags,
                    default: descriptor
                        .default_properties
                        .get(*property_name)
                        .and_then(|value| serde_json::to_value(value).ok()),
                    summary: doc_property.and_then(|member| member.summary.clone()),
                    description: doc_property.and_then(|member| member.description.clone()),
                    deprecated: doc_property.is_some_and(Member::deprecated),
                    deprecation_message: doc_property
                        .and_then(|member| member.deprecation_message.clone()),
                    security: doc_property.and_then(|member| member.security.clone()),
                    thread_safety: doc_property.and_then(|member| member.thread_safety.clone()),
                    docs_serialization: doc_property
                        .and_then(|member| member.serialization.clone()),
                    capabilities: doc_property
                        .map(|member| member.capabilities.clone())
                        .unwrap_or_default(),
                },
            );
        }
        properties
    }

    fn add_enums(&mut self) {
        let mut enums = self.database.enums.iter().collect::<Vec<_>>();
        enums.sort_by_key(|(name, _)| **name);
        for (name, descriptor) in enums {
            self.add_enum(name, descriptor);
        }
    }

    fn add_enum(&mut self, name: &str, descriptor: &EnumDescriptor<'_>) {
        let doc = self.docs.enums.get(name);
        let enum_classification = match doc {
            Some(doc) if doc.kind != "enum" => {
                self.diagnostics.push(Diagnostic {
                    name: name.to_owned(),
                    classification: Classification::MetadataConflict,
                    reflection: "enum".to_owned(),
                    api: doc.kind.clone(),
                });
                Classification::MetadataConflict
            }
            Some(_) => Classification::Matched,
            None => Classification::ReflectionOnly,
        };
        self.coverage.push(item(
            "reflection+api",
            "enum",
            name,
            enum_classification,
            true,
            "enum-backed property values",
            Some(definition_ref(&["enum", name])),
        ));
        let mut items = BTreeMap::new();
        let mut reflection_items = descriptor.items.iter().collect::<Vec<_>>();
        reflection_items.sort_by_key(|(name, _)| **name);
        for (item_name, value) in reflection_items {
            let doc_item = doc.and_then(|doc| doc.member("items", item_name));
            let classification = match doc_item {
                Some(item) if item.value == Some(*value) => Classification::Matched,
                Some(_) => Classification::MetadataConflict,
                None => Classification::ReflectionOnly,
            };
            if classification == Classification::MetadataConflict {
                self.diagnostics.push(Diagnostic {
                    name: format!("{name}.{item_name}"),
                    classification,
                    reflection: value.to_string(),
                    api: doc_item
                        .and_then(|item| item.value)
                        .map_or_else(|| "missing".to_owned(), |value| value.to_string()),
                });
            }
            self.coverage.push(item(
                "reflection+api",
                "enum-item",
                &format!("{name}.{item_name}"),
                classification,
                true,
                "enumerated property value",
                Some(definition_ref(&["enum", name])),
            ));
            items.insert(
                (*item_name).to_owned(),
                EnumItem {
                    name: (*item_name).to_owned(),
                    value: *value,
                    summary: doc_item.and_then(|item| item.summary.clone()),
                    deprecated: doc_item.is_some_and(Member::deprecated),
                    deprecation_message: doc_item.and_then(|item| item.deprecation_message.clone()),
                },
            );
        }
        self.enums.insert(
            name.to_owned(),
            Enum {
                name: name.to_owned(),
                summary: doc.and_then(|doc| doc.summary.clone()),
                description: doc.and_then(|doc| doc.description.clone()),
                deprecated: doc.is_some_and(Doc::deprecated),
                deprecation_message: doc.and_then(|doc| doc.deprecation_message.clone()),
                items,
            },
        );
    }

    fn add_variant_coverage(&mut self) {
        for variant in &self.variants {
            let supported = self.supported.contains(variant.as_str());
            self.coverage.push(item(
                "reflection",
                "variant-type",
                variant,
                if supported {
                    Classification::Matched
                } else {
                    Classification::Unsupported
                },
                supported,
                if supported {
                    "exact explicit tagged value schema"
                } else {
                    "no exact representation available"
                },
                supported.then(|| definition_ref(&["value", variant])),
            ));
        }
    }

    fn finish(mut self) -> Api {
        self.coverage.sort_by(|left, right| {
            (&left.kind, &left.name, &left.source).cmp(&(&right.kind, &right.name, &right.source))
        });
        self.diagnostics
            .sort_by(|left, right| left.name.cmp(&right.name));
        let reflection_version = self
            .database
            .version
            .iter()
            .map(u32::to_string)
            .collect::<Vec<_>>()
            .join(".");
        let docs_counts = BTreeMap::from([
            ("classes".to_owned(), self.docs.classes.len()),
            ("datatypes".to_owned(), self.docs.datatypes.len()),
            ("enums".to_owned(), self.docs.enums.len()),
            ("globals".to_owned(), self.docs.globals.len()),
            ("libraries".to_owned(), self.docs.libraries.len()),
        ]);

        Api {
            studio_version: self.docs.studio_version.clone(),
            reflection_version,
            classes: self.classes,
            enums: self.enums,
            variant_types: self.variants.into_iter().collect(),
            coverage: self.coverage,
            diagnostics: self.diagnostics,
            docs_counts,
        }
    }
}

fn property_kind(kind: &PropertyKind<'_>) -> (String, String, Option<String>, Vec<String>) {
    match kind {
        PropertyKind::Alias { alias_for } => (
            "alias".to_owned(),
            "alias".to_owned(),
            Some((*alias_for).to_owned()),
            Vec::new(),
        ),
        PropertyKind::Canonical { serialization } => {
            let (name, migrations) = match serialization {
                PropertySerialization::Serializes => ("serializes".to_owned(), Vec::new()),
                PropertySerialization::DoesNotSerialize => {
                    ("does-not-serialize".to_owned(), Vec::new())
                }
                PropertySerialization::SerializesAs(name) => {
                    (format!("serializes-as:{name}"), Vec::new())
                }
                PropertySerialization::Migrate(migration) => (
                    format!("migrate:{migration:?}"),
                    migration
                        .new_property_names()
                        .iter()
                        .map(|name| (*name).to_owned())
                        .collect(),
                ),
                _ => ("unknown".to_owned(), Vec::new()),
            };
            ("canonical".to_owned(), name, None, migrations)
        }
        _ => ("unknown".to_owned(), "unknown".to_owned(), None, Vec::new()),
    }
}

struct PropertyCheck<'a> {
    class_name: &'a str,
    property_name: &'a str,
    data_type: &'a PropertyType,
    serialization: &'a str,
    doc: Option<&'a Member>,
    supported: bool,
}

fn classify_property(
    check: &PropertyCheck<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) -> (Classification, String) {
    if !check.supported {
        return (
            Classification::Unsupported,
            "unsupported reflected type".to_owned(),
        );
    }
    let Some(doc) = check.doc else {
        return (
            Classification::ReflectionOnly,
            "property-specific schema".to_owned(),
        );
    };
    if !doc_type_matches(doc.data_type.as_deref(), check.data_type) {
        let api_type = doc
            .data_type
            .clone()
            .unwrap_or_else(|| "<missing>".to_owned());
        diagnostics.push(Diagnostic {
            name: format!("{}.{}", check.class_name, check.property_name),
            classification: Classification::TypeConflict,
            reflection: check.data_type.name().to_owned(),
            api: api_type,
        });
        return (
            Classification::TypeConflict,
            "reflection type takes precedence".to_owned(),
        );
    }
    if metadata_conflicts(check.serialization, doc.serialization.as_ref()) {
        diagnostics.push(Diagnostic {
            name: format!("{}.{}", check.class_name, check.property_name),
            classification: Classification::MetadataConflict,
            reflection: check.serialization.to_owned(),
            api: doc
                .serialization
                .as_ref()
                .map_or_else(|| "<missing>".to_owned(), ValueExt::compact),
        });
        return (
            Classification::MetadataConflict,
            "reflection serialization takes precedence; API metadata retained".to_owned(),
        );
    }
    (
        Classification::Matched,
        "property-specific schema".to_owned(),
    )
}

trait ValueExt {
    fn compact(&self) -> String;
}

impl ValueExt for serde_json::Value {
    fn compact(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| "<invalid>".to_owned())
    }
}

fn metadata_conflicts(serialization: &str, docs: Option<&serde_json::Value>) -> bool {
    let reflection_serializes = serialization != "does-not-serialize";
    let docs_can_load = docs
        .and_then(|value| value.get("can_load"))
        .and_then(serde_json::Value::as_bool);
    matches!(docs_can_load, Some(can_load) if can_load != reflection_serializes)
}

fn doc_type_matches(docs: Option<&str>, reflection: &PropertyType) -> bool {
    let Some(docs) = docs else {
        return false;
    };
    let docs = docs.trim_end_matches('?').trim_start_matches("Enum.");
    matches!(reflection, PropertyType::Enum(name) | PropertyType::Value(name) if docs == name)
}

fn add_docs_class_coverage(
    docs: &Catalog,
    reflection: &BTreeMap<String, Class>,
    coverage: &mut Vec<CoverageItem>,
) {
    for (name, doc) in &docs.classes {
        if !reflection.contains_key(name) {
            coverage.push(item(
                "api",
                "class",
                name,
                Classification::ApiOnly,
                false,
                "not present in Rojo's pinned reflection database",
                None,
            ));
        }
        for property in &doc.members["properties"] {
            let short = property
                .name
                .rsplit_once('.')
                .map_or(property.name.as_str(), |(_, name)| name);
            if reflection
                .get(name)
                .is_none_or(|class| !class.properties.contains_key(short))
            {
                coverage.push(item(
                    "api",
                    "property",
                    &property.name,
                    Classification::ApiOnly,
                    false,
                    "not present in Rojo's pinned reflection database",
                    None,
                ));
            }
        }
        for kind in ["methods", "events", "callbacks"] {
            for member in &doc.members[kind] {
                coverage.push(item(
                    "api",
                    kind.trim_end_matches('s'),
                    &member.name,
                    Classification::NonProjectable,
                    false,
                    "runtime API member, not a project property",
                    None,
                ));
            }
        }
    }
}

fn add_docs_enum_coverage(
    docs: &Catalog,
    reflection: &BTreeMap<String, Enum>,
    coverage: &mut Vec<CoverageItem>,
) {
    for (name, doc) in &docs.enums {
        if !reflection.contains_key(name) {
            coverage.push(item(
                "api",
                "enum",
                name,
                Classification::ApiOnly,
                false,
                "not present in Rojo's pinned reflection database",
                None,
            ));
        }
        for enum_item in &doc.members["items"] {
            if reflection
                .get(name)
                .is_none_or(|item| !item.items.contains_key(&enum_item.name))
            {
                coverage.push(item(
                    "api",
                    "enum-item",
                    &format!("{name}.{}", enum_item.name),
                    Classification::ApiOnly,
                    false,
                    "not present in Rojo's pinned reflection database",
                    None,
                ));
            }
        }
    }
}

fn add_non_projectable_docs(docs: &Catalog, coverage: &mut Vec<CoverageItem>) {
    for (kind, catalog) in [
        ("datatype", &docs.datatypes),
        ("global", &docs.globals),
        ("library", &docs.libraries),
    ] {
        for (name, doc) in catalog {
            coverage.push(item(
                "api",
                kind,
                name,
                Classification::NonProjectable,
                false,
                &format!("{} runtime API surface, not an Instance property", doc.kind),
                None,
            ));
            for (member_kind, members) in &doc.members {
                for member in members {
                    coverage.push(item(
                        "api",
                        &format!("{kind}-{}", singular(member_kind)),
                        &member.name,
                        Classification::NonProjectable,
                        false,
                        "runtime API surface, not an Instance property",
                        None,
                    ));
                }
            }
        }
    }
}

fn item(
    source: &str,
    kind: &str,
    name: &str,
    classification: Classification,
    projectable: bool,
    disposition: &str,
    schema_ref: Option<String>,
) -> CoverageItem {
    CoverageItem {
        source: source.to_owned(),
        kind: kind.to_owned(),
        name: name.to_owned(),
        classification,
        projectable,
        disposition: disposition.to_owned(),
        schema_ref,
    }
}

fn definition_ref(parts: &[&str]) -> String {
    crate::pointer::path(&parts.join("/"))
}

fn singular(kind: &str) -> &str {
    if kind == "properties" {
        "property"
    } else {
        kind.strip_suffix('s').unwrap_or(kind)
    }
}

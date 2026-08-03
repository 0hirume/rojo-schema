use std::collections::{BTreeMap, BTreeSet};

use anyhow::{bail, Context, Result};
use serde_json::{json, Map, Value};

use crate::{
    format::Formats,
    grammar::{Grammar, Node},
    model::{Api, Class, Property, PropertyType},
};

pub const DRAFT: &str = "https://json-schema.org/draft/2020-12/schema";

pub struct Output {
    pub value: Value,
    pub flattened_properties: usize,
    pub definitions: usize,
}

pub fn build(
    api: &Api,
    rojo_version: &str,
    grammar: &Grammar,
    formats: &Formats,
) -> Result<Output> {
    let schema_id = format!(
        "{}/latest/rojo.schema.json",
        env!("CARGO_PKG_HOMEPAGE").trim_end_matches('/')
    );
    let mut definitions = formats.definitions.clone();
    definitions.extend(grammar.definitions.clone());
    link_external_formats(&mut definitions, formats);
    definitions.extend(enum_definitions(api));
    definitions.extend(property_definitions(api, grammar, &formats.enum_variant));
    let (properties, flattened_properties) = properties_definitions(api, &grammar.node)?;
    definitions.extend(properties);
    definitions.extend(node_definitions(api, &grammar.node)?);
    let definition_count = definitions.len();

    let mut schema = grammar.root.clone();
    let root = schema
        .as_object_mut()
        .context("Rojo Project must serialize as an object")?;
    root.insert("$schema".to_owned(), json!(DRAFT));
    root.insert("$id".to_owned(), json!(schema_id));
    root.insert("title".to_owned(), json!("Rojo project"));
    root.insert(
        "description".to_owned(),
        json!(format!(
            "Rojo {rojo_version} project file targeting bundled Roblox reflection {} and Creator Docs Studio {}.",
            api.reflection_version, api.studio_version
        )),
    );
    root.get_mut("properties")
        .and_then(Value::as_object_mut)
        .context("Rojo Project schema has no property map")?
        .insert(grammar.tree.clone(), reference("node/Any"));
    root.insert("$defs".to_owned(), Value::Object(definitions));

    Ok(Output {
        value: schema,
        flattened_properties,
        definitions: definition_count,
    })
}

fn link_external_formats(definitions: &mut Map<String, Value>, formats: &Formats) {
    let names = definitions
        .keys()
        .filter_map(|key| key.strip_prefix("serde/"))
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    for name in names {
        let rojo = format!("rojo/{name}");
        if definitions.contains_key(&rojo) {
            definitions.insert(rojo, reference(&format!("serde/{name}")));
        }
    }
    let rojo_root = format!("rojo/{}", formats.root);
    if definitions.contains_key(&rojo_root) {
        definitions.insert(rojo_root, reference("value/Any"));
    }
}

fn enum_definitions(api: &Api) -> Map<String, Value> {
    api.enums
        .iter()
        .map(|(name, descriptor)| {
            let mut schema = json!({
                "type": "string",
                "enum": descriptor.items.keys().collect::<Vec<_>>(),
                "title": descriptor.name
            });
            annotate(
                &mut schema,
                descriptor.summary.as_deref(),
                descriptor.description.as_deref(),
                descriptor.deprecated,
                descriptor.deprecation_message.as_deref(),
            );
            (format!("enum/{name}"), schema)
        })
        .collect()
}

fn property_definitions(api: &Api, grammar: &Grammar, enum_variant: &str) -> Map<String, Value> {
    api.classes
        .values()
        .flat_map(|class| {
            class.properties.values().map(|property| {
                (
                    format!("property/{}/{}", property.owner, property.name),
                    property_schema(
                        api,
                        property,
                        &grammar.compact,
                        &grammar.definitions,
                        enum_variant,
                    ),
                )
            })
        })
        .collect()
}

fn properties_definitions(api: &Api, node: &Node) -> Result<(Map<String, Value>, usize)> {
    let mut definitions = Map::new();
    let mut count = 0;
    for (name, class) in &api.classes {
        let properties = flattened(api, class)?
            .into_iter()
            .filter(|(_, property)| supported(api, property))
            .map(|(name, property)| {
                (
                    name,
                    reference(&format!("property/{}/{}", property.owner, property.name)),
                )
            })
            .collect::<Map<_, _>>();
        count += properties.len();
        definitions.insert(
            format!("properties/{name}"),
            json!({
                "type": "object",
                "properties": properties,
                "additionalProperties": false,
                "default": {}
            }),
        );
    }

    let names = api
        .classes
        .values()
        .flat_map(|class| class.properties.keys().cloned())
        .collect::<BTreeSet<_>>();
    definitions.insert(
        "properties/Path".to_owned(),
        json!({
            "type": "object",
            "propertyNames": { "enum": names },
            "additionalProperties": reference(&format!("rojo/{}", node.value)),
            "default": {}
        }),
    );
    Ok((definitions, count))
}

fn node_definitions(api: &Api, node: &Node) -> Result<Map<String, Value>> {
    let mut definitions = Map::new();
    for (name, class) in &api.classes {
        let mut schema = node.schema.clone();
        configure_node(&mut schema, node, Some(name), &format!("properties/{name}"))?;
        annotate(
            &mut schema,
            class.summary.as_deref(),
            class.description.as_deref(),
            class.deprecated,
            class.deprecation_message.as_deref(),
        );
        definitions.insert(format!("node/{name}"), schema);
    }

    let mut path = node.schema.clone();
    configure_node(&mut path, node, None, "properties/Path")?;
    definitions.insert("node/Path".to_owned(), path);
    definitions.insert(
        "node/Any".to_owned(),
        json!({
            "anyOf": api
                .classes
                .keys()
                .map(|name| reference(&format!("node/{name}")))
                .chain(std::iter::once(reference("node/Path")))
                .collect::<Vec<_>>()
        }),
    );
    Ok(definitions)
}

fn configure_node(
    schema: &mut Value,
    node: &Node,
    class: Option<&str>,
    properties: &str,
) -> Result<()> {
    let object = schema
        .as_object_mut()
        .context("Rojo ProjectNode must serialize as an object")?;
    if class.is_some() {
        object.insert("required".to_owned(), json!([node.class]));
    }
    let fields = object
        .get_mut("properties")
        .and_then(Value::as_object_mut)
        .context("Rojo ProjectNode schema has no property map")?;
    if let Some(class) = class {
        fields.insert(node.class.clone(), json!({ "const": class }));
    }
    fields.insert(node.properties.clone(), reference(properties));
    object.insert("additionalProperties".to_owned(), reference("node/Any"));
    Ok(())
}

fn supported(api: &Api, property: &Property) -> bool {
    match &property.data_type {
        PropertyType::Enum(name) => api.enums.contains_key(name),
        PropertyType::Value(name) => api.variant_types.binary_search(name).is_ok(),
    }
}

fn flattened<'a>(api: &'a Api, class: &'a Class) -> Result<BTreeMap<String, &'a Property>> {
    let mut hierarchy = Vec::new();
    let mut current = Some(class);
    let mut seen = BTreeSet::new();
    while let Some(item) = current {
        if !seen.insert(item.name.as_str()) {
            bail!("reflection inheritance cycle at {}", item.name);
        }
        hierarchy.push(item);
        current = item
            .superclass
            .as_ref()
            .and_then(|name| api.classes.get(name));
    }
    hierarchy.reverse();

    let mut properties = BTreeMap::new();
    for item in hierarchy {
        for (name, property) in &item.properties {
            properties.insert(name.clone(), property);
        }
    }
    Ok(properties)
}

fn property_schema(
    api: &Api,
    property: &Property,
    compact: &BTreeMap<String, Value>,
    definitions: &Map<String, Value>,
    enum_variant: &str,
) -> Value {
    let (mut schema, default) = match &property.data_type {
        PropertyType::Enum(name) => (
            choice(vec![
                reference(&format!("enum/{name}")),
                reference(&format!("value/{enum_variant}")),
            ]),
            enum_default(api, name, enum_variant, property.default.as_ref()),
        ),
        PropertyType::Value(name) => {
            let qualified = reference(&format!("value/{name}"));
            if let Some(schema) = compact.get(name) {
                (
                    choice(vec![schema.clone(), qualified]),
                    compact_default(name, property.default.as_ref(), schema, definitions),
                )
            } else {
                (qualified, property.default.clone())
            }
        }
    };
    annotate(
        &mut schema,
        property.summary.as_deref(),
        property.description.as_deref(),
        property.deprecated,
        property.deprecation_message.as_deref(),
    );
    let object = schema.as_object_mut().expect("property schema object");
    object.insert("title".to_owned(), json!(property.name));
    object.insert("x-rojo-owner".to_owned(), json!(property.owner));
    object.insert("x-rojo-kind".to_owned(), json!(property.kind));
    object.insert(
        "x-rojo-serialization".to_owned(),
        json!(property.serialization),
    );
    object.insert(
        "x-rojo-scriptability".to_owned(),
        json!(property.scriptability),
    );
    object.insert("x-roblox-tags".to_owned(), json!(property.tags));
    if let Some(alias) = &property.alias_for {
        object.insert("x-rojo-alias-for".to_owned(), json!(alias));
    }
    if !property.migration_targets.is_empty() {
        object.insert(
            "x-rojo-migration-targets".to_owned(),
            json!(property.migration_targets),
        );
    }
    if let Some(default) = default {
        object.insert("default".to_owned(), default);
    }
    if let Some(security) = &property.security {
        object.insert("x-roblox-security".to_owned(), security.clone());
    }
    if let Some(thread_safety) = &property.thread_safety {
        object.insert("x-roblox-thread-safety".to_owned(), json!(thread_safety));
    }
    if let Some(serialization) = &property.docs_serialization {
        object.insert("x-roblox-serialization".to_owned(), serialization.clone());
    }
    if !property.capabilities.is_empty() {
        object.insert(
            "x-roblox-capabilities".to_owned(),
            json!(property.capabilities),
        );
    }
    schema
}

fn enum_default(api: &Api, name: &str, variant: &str, default: Option<&Value>) -> Option<Value> {
    let value = default?.get(variant)?.as_u64()?;
    api.enums
        .get(name)?
        .items
        .values()
        .find(|item| u64::from(item.value) == value)
        .map(|item| json!(item.name))
}

fn compact_default(
    name: &str,
    default: Option<&Value>,
    schema: &Value,
    definitions: &Map<String, Value>,
) -> Option<Value> {
    let mut value = default?.get(name)?.clone();
    loop {
        if accepts(&value, schema, definitions) {
            return Some(value);
        }
        let object = value.as_object()?;
        if object.len() != 1 {
            return None;
        }
        value = object.values().next()?.clone();
    }
}

fn accepts(value: &Value, schema: &Value, definitions: &Map<String, Value>) -> bool {
    let Some(schema) = schema.as_object() else {
        return schema.as_bool().unwrap_or(false);
    };
    if schema.contains_key("$ref") {
        return definitions.iter().any(|(name, target)| {
            Value::Object(schema.clone()) == reference(name) && accepts(value, target, definitions)
        });
    }
    if let Some(expected) = schema.get("const") {
        if value != expected {
            return false;
        }
    }
    if let Some(options) = schema.get("anyOf").and_then(Value::as_array) {
        if !options
            .iter()
            .any(|option| accepts(value, option, definitions))
        {
            return false;
        }
    }
    if let Some(options) = schema.get("allOf").and_then(Value::as_array) {
        if !options
            .iter()
            .all(|option| accepts(value, option, definitions))
        {
            return false;
        }
    }
    match schema.get("type").and_then(Value::as_str) {
        Some("null") => value.is_null(),
        Some("boolean") => value.is_boolean(),
        Some("string") => value.is_string(),
        Some("number") => value.is_number(),
        Some("integer") => value.as_i64().is_some() || value.as_u64().is_some(),
        Some("array") => array_accepts(value, schema, definitions),
        Some("object") => value.is_object(),
        Some(_) => false,
        None => true,
    }
}

fn array_accepts(
    value: &Value,
    schema: &Map<String, Value>,
    definitions: &Map<String, Value>,
) -> bool {
    let Some(items) = value.as_array() else {
        return false;
    };
    if schema
        .get("minItems")
        .and_then(Value::as_u64)
        .is_some_and(|minimum| items.len() < usize::try_from(minimum).unwrap_or(usize::MAX))
        || schema
            .get("maxItems")
            .and_then(Value::as_u64)
            .is_some_and(|maximum| items.len() > usize::try_from(maximum).unwrap_or(0))
    {
        return false;
    }
    schema
        .get("items")
        .is_none_or(|item| items.iter().all(|value| accepts(value, item, definitions)))
}

fn choice(mut options: Vec<Value>) -> Value {
    if options.len() == 1 {
        options.pop().expect("one option")
    } else {
        json!({ "anyOf": options })
    }
}

fn annotate(
    schema: &mut Value,
    summary: Option<&str>,
    description: Option<&str>,
    deprecated: bool,
    deprecation_message: Option<&str>,
) {
    let object = schema.as_object_mut().expect("schema object");
    let combined = match (summary, description) {
        (Some(summary), Some(description)) if summary != description => {
            Some(format!("{summary}\n\n{description}"))
        }
        (Some(summary), _) => Some(summary.to_owned()),
        (_, Some(description)) => Some(description.to_owned()),
        _ => None,
    };
    if let Some(description) = combined {
        object.insert("description".to_owned(), json!(description));
    }
    if deprecated {
        object.insert("deprecated".to_owned(), json!(true));
    }
    if let Some(message) = deprecation_message.filter(|message| !message.is_empty()) {
        object.insert("x-roblox-deprecation-message".to_owned(), json!(message));
    }
}

fn reference(key: &str) -> Value {
    crate::pointer::reference(key)
}

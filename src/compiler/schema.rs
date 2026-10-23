use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write as _,
    iter,
};

use eyre::{ContextCompat as _, Result, bail, ensure};
use serde_json::{Map, Value, json};

use super::{
    format::Formats,
    grammar::{Field, Grammar, Model, Node},
    inference,
    model::{Api, Class, Property, PropertyType},
    pointer, presentation,
};

pub(super) const DRAFT: &str = "https://json-schema.org/draft/2020-12/schema";

pub(super) struct Output {
    pub project: Value,
    pub model: Value,
    pub flattened_properties: usize,
    pub project_definitions: usize,
    pub model_definitions: usize,
}

pub(super) fn build(
    api: &Api,
    rojo_version: &str,
    grammar: &Grammar,
    formats: &Formats,
    docs: &super::docs::Catalog,
) -> Result<Output> {
    let homepage = env!("CARGO_PKG_HOMEPAGE").trim_end_matches('/');
    let project_id = format!("{homepage}/latest/project.schema.json");
    let model_id = format!("{homepage}/latest/model.schema.json");
    let mut definitions = formats.definitions.clone();
    definitions.extend(grammar.definitions.clone());
    link_external_formats(&mut definitions, formats);

    datatype_annotations(&mut definitions, docs, api);

    if let Some(value) = definitions
        .get_mut(&format!("rojo/{}", grammar.node.value))
        .and_then(Value::as_object_mut)
    {
        for annotation in ["description", "markdownDescription", "title"] {
            value.remove(annotation);
        }
    }

    definitions.extend(enum_definitions(api));
    definitions.extend(property_definitions(api, grammar, &formats.enum_variant));
    let (properties, flattened_properties) = properties_definitions(api)?;
    definitions.extend(properties);

    let mut project_definitions = definitions.clone();
    project_definitions.extend(node_definitions(api, grammar)?);
    let mut project = grammar.project.clone();

    let root = project
        .as_object_mut()
        .context("Rojo Project must serialize as an object")?;

    root.insert("$schema".to_owned(), json!(DRAFT));
    root.insert("$id".to_owned(), json!(project_id));

    root.insert(
        "description".to_owned(),
        json!(format!(
            "Rojo {rojo_version} project file targeting Roblox reflection {} and Creator Docs Studio {}.",
            api.reflection_version, api.studio_version
        )),
    );

    let fields = root
        .get_mut("properties")
        .and_then(Value::as_object_mut)
        .context("Rojo Project schema has no property map")?;

    let mut tree = reference("node/Any");

    annotate(
        &mut tree,
        Some(
            "The root instance and its descendants. Child keys name instances; `$`-prefixed keys configure each instance.",
        ),
        None,
        false,
        None,
    );

    fields.insert(grammar.tree.clone(), tree);

    if let Some(schema) = fields.get_mut("$schema") {
        annotate(
            schema,
            Some(
                "The schema URI used by your editor for completion and validation. Local absolute paths must use a `file:///` URI.",
            ),
            None,
            false,
            None,
        );
    }

    root.insert("$defs".to_owned(), Value::Object(project_definitions));

    let mut model_definitions = definitions;
    model_definitions.extend(model_definitions_for(api, grammar)?);
    let model_definition_count = model_definitions.len();

    let mut model = json!({
        "$schema": DRAFT,
        "$id": model_id,
        "description": format!(
            "Rojo {rojo_version} JSON model file targeting Roblox reflection {} and Creator Docs Studio {}.",
            api.reflection_version, api.studio_version
        ),
        "$ref": "#/$defs/model~1Any",
        "$defs": model_definitions
    });

    inference::apply(&mut project, grammar)?;

    let project_definition_count = project["$defs"]
        .as_object()
        .context("project definitions are missing")?
        .len();

    presentation::schema(&mut project, "project");
    presentation::schema(&mut model, "model");

    Ok(Output {
        project,
        model,
        flattened_properties,
        project_definitions: project_definition_count,
        model_definitions: model_definition_count,
    })
}

fn datatype_annotations(
    definitions: &mut Map<String, Value>,
    docs: &super::docs::Catalog,
    api: &Api,
) {
    for (name, doc) in &docs.datatypes {
        if let Some(schema) = definitions.get_mut(&format!("serde/{name}")) {
            annotate(
                schema,
                doc.summary.as_deref(),
                doc.description.as_deref(),
                doc.deprecated(),
                doc.deprecation_message.as_deref(),
            );
        }

        if let Some(value) = definitions
            .get_mut(&format!("value/{name}"))
            .and_then(|schema| schema.get_mut("properties"))
            .and_then(|fields| fields.get_mut(name))
        {
            annotate(
                value,
                doc.summary.as_deref(),
                doc.description.as_deref(),
                doc.deprecated(),
                doc.deprecation_message.as_deref(),
            );
        }
    }

    for property in api
        .classes
        .values()
        .flat_map(|class| class.properties.values())
    {
        let PropertyType::Value(name) = &property.data_type else {
            continue;
        };

        let Some(default) = &property.default else {
            continue;
        };

        let Some(schema) = definitions
            .get_mut(&format!("value/{name}"))
            .and_then(Value::as_object_mut)
        else {
            continue;
        };

        schema.entry("examples").or_insert_with(|| json!([default]));

        schema
            .entry("defaultSnippets")
            .or_insert_with(|| json!([{ "label": "Reflected value", "body": default }]));

        if let Some(value) = schema
            .get_mut("properties")
            .and_then(|fields| fields.get_mut(name))
            .and_then(Value::as_object_mut)
            && let Some(payload) = default.get(name)
        {
            value.entry("examples").or_insert_with(|| json!([payload]));

            value
                .entry("defaultSnippets")
                .or_insert_with(|| json!([{ "label": "Reflected value", "body": payload }]));
        }
    }
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
            let descriptions = descriptor.items.values().map(|item| {
                let mut description = String::new();
                let summary = item.summary.as_deref();

                let detail = item.description.as_deref().or_else(|| {
                    if summary.is_none() {
                        descriptor.summary.as_deref().or(descriptor.description.as_deref())
                    } else {
                        None
                    }
                });

                let mut documentation = json!({});
                annotate(&mut documentation, summary, detail, false, None);

                if let Some(text) = documentation.get("markdownDescription").and_then(Value::as_str) {
                    description.push_str(text);
                    description.push_str("\n\n");
                }

                write!(description, "Numeric value: `{}`.", item.value).expect("formatting a string");

                if item.deprecated || descriptor.deprecated {
                    description.push_str("\n\n**Deprecated**");

                    if let Some(message) = item.deprecation_message.as_deref().or(descriptor.deprecation_message.as_deref()) {
                        description.push_str("\n\n");
                        description.push_str(message);
                    }
                }

                description
            }).collect::<Vec<_>>();

            let schema = json!({
                "type": "string",
                "enum": descriptor.items.keys().collect::<Vec<_>>(),
                "enumDescriptions": descriptions.iter().map(|text| presentation::plaintext(text)).collect::<Vec<_>>(),
                "markdownEnumDescriptions": descriptions
            });

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

fn properties_definitions(api: &Api) -> Result<(Map<String, Value>, usize)> {
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

        let mut schema = json!({
            "type": "object",
            "properties": properties,
            "additionalProperties": false,
            "default": {}
        });

        annotate(
            &mut schema,
            Some(&format!(
                "Property values for `{name}` instances, including inherited properties."
            )),
            None,
            false,
            None,
        );

        definitions.insert(format!("properties/{name}"), schema);
    }

    Ok((definitions, count))
}

fn node_definitions(api: &Api, grammar: &Grammar) -> Result<Map<String, Value>> {
    let node = &grammar.node;
    let inferred = inferred_children(api, grammar)?;
    let mut definitions = Map::new();

    for (name, class) in &api.classes {
        let mut schema = node.schema.clone();

        configure_node(
            &mut schema,
            node,
            Some(name),
            reference(&format!("properties/{name}")),
        )?;

        add_children(&mut schema, inferred.get(name))?;

        annotate(
            &mut schema,
            class.summary.as_deref(),
            class.description.as_deref(),
            class.deprecated,
            class.deprecation_message.as_deref(),
        );

        schema["title"] = json!(name);
        schema["x-rojo-class"] = json!(name);

        definitions.insert(format!("node/{name}"), schema);
    }

    let mut generic = node.schema.clone();
    configure_node(&mut generic, node, None, generic_properties(api, node))?;

    definitions.insert(
        "node/Any".to_owned(),
        node_dispatch(generic, api, node, &inferred)?,
    );

    for child in inferred.values().flatten() {
        definitions.insert(format!("inferred/{child}"), json!({
            "if": { "anyOf": [
                { "properties": { node.class.clone(): { "type": "string" } }, "required": [node.class] },
                { "properties": { "$path": { "not": { "type": "null" } } }, "required": ["$path"] }
            ] },
            "then": reference("node/Any"),
            "else": reference(&format!("node/{child}"))
        }));
    }

    Ok(definitions)
}

fn model_definitions_for(api: &Api, grammar: &Grammar) -> Result<Map<String, Value>> {
    let model = &grammar.model;
    let mut definitions = Map::new();

    for (name, class) in &api.classes {
        let mut schema = model.schema.clone();

        configure_model(
            &mut schema,
            model,
            Some(name),
            &format!("properties/{name}"),
        )?;

        annotate(
            &mut schema,
            class.summary.as_deref(),
            class.description.as_deref(),
            class.deprecated,
            class.deprecation_message.as_deref(),
        );

        schema["title"] = json!(name);
        schema["x-rojo-class"] = json!(name);

        definitions.insert(format!("model/{name}"), schema);
    }

    definitions.insert("model/Any".to_owned(), model_union(api));

    Ok(definitions)
}

fn configure_model(
    schema: &mut Value,
    model: &Model,
    class: Option<&str>,
    properties: &str,
) -> Result<()> {
    let fields = schema
        .get_mut("properties")
        .and_then(Value::as_object_mut)
        .context("Rojo JsonModel schema has no property map")?;

    if let Some(class) = class {
        set_model_field(fields, &model.class, &json!({ "const": class }));
    }

    set_model_field(fields, &model.properties, &reference(properties));

    set_model_field(
        fields,
        &model.children,
        &json!({ "type": "array", "items": reference("model/Any") }),
    );

    for (field, description) in [
        (
            &model.class,
            "The Roblox class of this instance. Determines which properties are available.",
        ),
        (
            &model.name,
            "The name of a child instance, defaulting to its class name. Rojo ignores this field on the root and uses the model file name.",
        ),
        (
            &model.properties,
            "Property values to assign to this instance. Available properties depend on `className`.",
        ),
        (
            &model.children,
            "Child instances parented to this instance. Each child is another model object.",
        ),
    ] {
        for name in model_field_names(field) {
            let schema = fields
                .get_mut(name)
                .context("model field schema is missing")?;

            annotate(schema, Some(description), None, false, None);
        }
    }

    for (name, description) in [
        (
            "attributes",
            "Custom Roblox attributes to assign to this instance.",
        ),
        (
            "id",
            "An identifier used to refer to this instance from referent properties.",
        ),
        (
            "$schema",
            "The schema URI used by your editor for completion and validation. Local absolute paths must use a `file:///` URI.",
        ),
    ] {
        if let Some(schema) = fields.get_mut(name) {
            annotate(schema, Some(description), None, false, None);
        }
    }

    Ok(())
}

fn model_union(api: &Api) -> Value {
    json!({
        "oneOf": api
            .classes
            .keys()
            .map(|name| reference(&format!("model/{name}")))
            .collect::<Vec<_>>()
    })
}

fn set_model_field(fields: &mut Map<String, Value>, field: &Field, schema: &Value) {
    for name in model_field_names(field) {
        fields.insert(name.to_owned(), schema.clone());
    }
}

fn model_field_names(field: &Field) -> impl Iterator<Item = &str> {
    iter::once(field.name.as_str()).chain(field.aliases.iter().map(String::as_str))
}

fn inferred_children(api: &Api, grammar: &Grammar) -> Result<BTreeMap<String, BTreeSet<String>>> {
    let mut output = BTreeMap::new();

    for (parent, inference) in &grammar.inferred {
        ensure!(
            api.classes.contains_key(parent),
            "Rojo infers children beneath unknown class {parent}"
        );

        let children = output.entry(parent.clone()).or_insert_with(BTreeSet::new);

        for name in &inference.names {
            ensure!(
                api.classes.contains_key(name),
                "Rojo infers unknown child class {name} beneath {parent}"
            );

            children.insert(name.clone());
        }

        for tag in &inference.tags {
            let mut found = false;

            for class in api
                .classes
                .values()
                .filter(|class| class.tags.contains(tag))
            {
                children.insert(class.name.clone());
                found = true;
            }

            ensure!(found, "Rojo infers {parent} children by unknown tag {tag}");
        }
    }

    Ok(output)
}

fn generic_properties(api: &Api, node: &Node) -> Value {
    let mut candidates: BTreeMap<&str, Vec<&Property>> = BTreeMap::new();

    for class in api.classes.values() {
        for property in class.properties.values() {
            candidates.entry(&property.name).or_default().push(property);
        }
    }

    let properties = candidates
        .into_iter()
        .map(|(name, candidates)| {
            let mut schema = reference(&format!("rojo/{}", node.value));

            let types = candidates
                .iter()
                .map(|property| property.data_type.name())
                .collect::<BTreeSet<_>>();

            let types = types.into_iter().collect::<Vec<_>>().join(" | ");
            schema["x-rojo-value-type"] = json!(types);

            let mut description = String::new();

            let summaries = candidates
                .iter()
                .filter_map(|property| {
                    property
                        .summary
                        .as_deref()
                        .or(property.description.as_deref())
                })
                .collect::<BTreeSet<_>>();

            for summary in summaries {
                description.push_str("\n\n");
                description.push_str(summary);
            }

            annotate(&mut schema, Some(&description), None, false, None);

            (name.to_owned(), schema)
        })
        .collect::<Map<_, _>>();

    json!({
        "type": "object",
        "propertyNames": { "enum": properties.keys().collect::<Vec<_>>() },
        "properties": properties,
        "additionalProperties": reference(&format!("rojo/{}", node.value)),
        "default": {}
    })
}

fn node_dispatch(
    mut generic: Value,
    api: &Api,
    node: &Node,
    inferred: &BTreeMap<String, BTreeSet<String>>,
) -> Result<Value> {
    let object = generic
        .as_object_mut()
        .context("Rojo ProjectNode must serialize as an object")?;

    let fields = object
        .get_mut("properties")
        .and_then(Value::as_object_mut)
        .context("Rojo ProjectNode schema has no property map")?;

    let properties = fields
        .remove(&node.properties)
        .context("Rojo ProjectNode properties field was not found")?;

    object.remove("additionalProperties");
    object.insert("unevaluatedProperties".to_owned(), reference("node/Any"));

    let branches = api
        .classes
        .keys()
        .map(|name| {
            let mut fields = Map::new();
            fields.insert(node.class.clone(), json!({ "const": name }));

            fields.insert(
                node.properties.clone(),
                reference(&format!("properties/{name}")),
            );

            add_child_fields(&mut fields, inferred.get(name));

            let class = &api.classes[name];

            let mut schema = json!({ "properties": fields, "title": name, "x-rojo-class": name });

            annotate(
                &mut schema,
                class.summary.as_deref().or(Some(name)),
                class.description.as_deref(),
                class.deprecated,
                class.deprecation_message.as_deref(),
            );

            schema
        })
        .collect::<Vec<_>>();

    let mut predicate = Map::new();
    predicate.insert(node.class.clone(), json!({ "type": "string" }));
    let mut fallback = Map::new();
    fallback.insert(node.class.clone(), json!({ "type": "null" }));
    fallback.insert(node.properties.clone(), properties);
    let mut fallback = json!({ "properties": fallback });

    for annotation in ["description", "markdownDescription", "title"] {
        if let Some(value) = object.remove(annotation) {
            fallback[annotation] = value;
        }
    }

    let dispatch = json!({
        "if": {
            "properties": predicate,
            "required": [node.class.clone()]
        },
        "then": { "oneOf": branches },
        "else": fallback
    });

    object
        .entry("allOf")
        .or_insert_with(|| Value::Array(Vec::new()))
        .as_array_mut()
        .context("Rojo ProjectNode allOf must be an array")?
        .push(dispatch);

    Ok(generic)
}

fn add_children(schema: &mut Value, children: Option<&BTreeSet<String>>) -> Result<()> {
    let fields = schema
        .get_mut("properties")
        .and_then(Value::as_object_mut)
        .context("Rojo ProjectNode schema has no property map")?;

    add_child_fields(fields, children);

    Ok(())
}

fn add_child_fields(fields: &mut Map<String, Value>, children: Option<&BTreeSet<String>>) {
    for child in children.into_iter().flatten() {
        fields.insert(child.clone(), reference(&format!("inferred/{child}")));
    }
}

fn configure_node(
    schema: &mut Value,
    node: &Node,
    class: Option<&str>,
    properties: Value,
) -> Result<()> {
    let object = schema
        .as_object_mut()
        .context("Rojo ProjectNode must serialize as an object")?;

    let fields = object
        .get_mut("properties")
        .and_then(Value::as_object_mut)
        .context("Rojo ProjectNode schema has no property map")?;

    if let Some(class) = class {
        fields.insert(node.class.clone(), json!({ "enum": [class, null] }));
    }

    fields.insert(node.properties.clone(), properties);

    for (name, description) in [
        (
            node.class.as_str(),
            "The Roblox class of this instance. Determines which properties are available.",
        ),
        (
            node.properties.as_str(),
            "Property values to assign to this instance. Available properties depend on `$className` or the class inferred from `$path`.",
        ),
        (
            "$attributes",
            "Custom Roblox attributes to assign to this instance.",
        ),
    ] {
        if let Some(schema) = fields.get_mut(name) {
            annotate(schema, Some(description), None, false, None);
        }
    }

    object.insert("additionalProperties".to_owned(), reference("node/Any"));

    Ok(())
}

fn supported(api: &Api, property: &Property) -> bool {
    match &property.data_type {
        PropertyType::Enum(name) => api.enums.contains_key(name),
        PropertyType::Value(name) => api.variant_types.binary_search(name).is_ok(),
    }
}

fn flattened<'source>(
    api: &'source Api,
    class: &'source Class,
) -> Result<BTreeMap<String, &'source Property>> {
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

    object.insert(
        "x-rojo-value-type".to_owned(),
        json!(property.data_type.name()),
    );

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
        object.insert("examples".to_owned(), json!([default]));

        object.insert(
            "defaultSnippets".to_owned(),
            json!([{ "label": "Reflected default", "body": default }]),
        );

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

    if let Some(capabilities) = &property.capabilities {
        object.insert("x-roblox-capabilities".to_owned(), capabilities.clone());
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

    if let Some(expected) = schema.get("const")
        && value != expected
    {
        return false;
    }

    if let Some(options) = schema.get("anyOf").and_then(Value::as_array)
        && !options
            .iter()
            .any(|option| accepts(value, option, definitions))
    {
        return false;
    }

    if let Some(options) = schema.get("allOf").and_then(Value::as_array)
        && !options
            .iter()
            .all(|option| accepts(value, option, definitions))
    {
        return false;
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
        (Some(summary), Some(description)) if description.starts_with(summary) => {
            Some(description.to_owned())
        }

        (Some(summary), Some(description)) if summary != description => {
            Some(format!("{summary}\n\n{description}"))
        }

        (Some(summary), _) => Some(summary.to_owned()),
        (_, Some(description)) => Some(description.to_owned()),
        _ => None,
    };

    if let Some(description) = combined {
        object.insert(
            "description".to_owned(),
            json!(presentation::plaintext(&description)),
        );

        object.insert("markdownDescription".to_owned(), json!(description));
    }

    if deprecated {
        object.insert("deprecated".to_owned(), json!(true));
    }

    if let Some(message) = deprecation_message.filter(|message| !message.is_empty()) {
        object.insert(
            "deprecationMessage".to_owned(),
            json!(presentation::plaintext(message)),
        );

        object.insert(
            "x-roblox-deprecation-message".to_owned(),
            json!(presentation::plaintext(message)),
        );
    }
}

fn reference(key: &str) -> Value {
    pointer::reference(key)
}

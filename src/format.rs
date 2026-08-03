use std::collections::{BTreeMap, BTreeSet};

use anyhow::{anyhow, ensure, Context, Result};
use rbx_types::VariantType;
use serde_json::{json, Map, Value};
use serde_reflection::{
    ContainerFormat, Format, Named, Registry, Samples, Tracer, TracerConfig, VariantFormat,
};

pub struct Formats {
    pub root: String,
    pub enum_variant: String,
    pub variants: BTreeSet<String>,
    pub definitions: Map<String, Value>,
}

pub fn values() -> Result<Formats> {
    let mut tracer = Tracer::new(
        TracerConfig::default()
            .is_human_readable(true)
            .default_borrowed_str_value("")
            .default_string_value(String::new()),
    );
    let (_, variant_types) = tracer
        .trace_simple_type::<VariantType>()
        .map_err(|error| anyhow!(error.to_string()))?;
    let variants = variant_types
        .iter()
        .map(|variant| format!("{variant:?}"))
        .collect::<BTreeSet<_>>();

    let mut samples = Samples::new();
    let mut root = None;
    let database = rbx_reflection_database::get_bundled();
    let mut classes = database.classes.iter().collect::<Vec<_>>();
    classes.sort_by_key(|(name, _)| **name);
    for (_, class) in classes {
        let mut properties = class.default_properties.iter().collect::<Vec<_>>();
        properties.sort_by_key(|(name, _)| **name);
        for (_, value) in properties {
            let (format, _) = tracer
                .trace_value(&mut samples, value)
                .map_err(|error| anyhow!(error.to_string()))?;
            if let Some(previous) = &root {
                ensure!(
                    previous == &format,
                    "reflected defaults use different root formats"
                );
            } else {
                root = Some(format);
            }
        }
    }

    let root = match root.context("reflection database has no default property values")? {
        Format::TypeName(name) => name,
        other => anyhow::bail!("reflected value root is not a named container: {other:?}"),
    };
    let registry = tracer.registry_unchecked();
    let definitions = definitions(&registry, &root, &variants)?;
    Ok(Formats {
        root,
        enum_variant: format!("{:?}", VariantType::Enum),
        variants,
        definitions,
    })
}

fn definitions(
    registry: &Registry,
    root: &str,
    variants: &BTreeSet<String>,
) -> Result<Map<String, Value>> {
    let mut definitions = Map::new();
    for (name, format) in registry {
        definitions.insert(serde_key(name), container(format));
    }

    let root_format = registry
        .get(root)
        .with_context(|| format!("missing traced root container {root}"))?;
    let ContainerFormat::Enum(traced) = root_format else {
        anyhow::bail!("traced root container {root} is not an enum");
    };
    let traced = traced
        .values()
        .map(|variant| (variant.name.as_str(), &variant.value))
        .collect::<BTreeMap<_, _>>();
    for name in variants {
        let schema = traced
            .get(name.as_str())
            .map_or_else(|| tagged(name, &json!({})), |format| variant(name, format));
        definitions.insert(value_key(name), schema);
    }
    definitions.insert(
        "value/Any".to_owned(),
        json!({
            "oneOf": variants
                .iter()
                .map(|name| reference(&value_key(name)))
                .collect::<Vec<_>>()
        }),
    );
    definitions.insert(serde_key(root), reference("value/Any"));
    Ok(definitions)
}

fn container(format: &ContainerFormat) -> Value {
    match format {
        ContainerFormat::UnitStruct => json!({ "type": "null" }),
        ContainerFormat::NewTypeStruct(format) => schema(format),
        ContainerFormat::TupleStruct(formats) => tuple(formats),
        ContainerFormat::Struct(fields) => structure(fields),
        ContainerFormat::Enum(variants) => {
            let options = variants
                .values()
                .map(|format| variant(&format.name, &format.value))
                .collect::<Vec<_>>();
            choice(options)
        }
    }
}

fn schema(format: &Format) -> Value {
    match format {
        Format::Variable(variable) => variable.borrow().as_ref().map_or_else(|| json!({}), schema),
        Format::TypeName(name) => reference(&serde_key(name)),
        Format::Unit => json!({ "type": "null" }),
        Format::Bool => json!({ "type": "boolean" }),
        Format::I8 => integer(i8::MIN, i8::MAX),
        Format::I16 => integer(i16::MIN, i16::MAX),
        Format::I32 => integer(i32::MIN, i32::MAX),
        Format::I64 => integer(i64::MIN, i64::MAX),
        Format::I128 => json!({ "type": "integer" }),
        Format::U8 => integer(u8::MIN, u8::MAX),
        Format::U16 => integer(u16::MIN, u16::MAX),
        Format::U32 => integer(u32::MIN, u32::MAX),
        Format::U64 => integer(u64::MIN, u64::MAX),
        Format::U128 => json!({ "type": "integer", "minimum": 0 }),
        Format::F32 | Format::F64 => json!({ "type": "number" }),
        Format::Char => json!({ "type": "string", "minLength": 1, "maxLength": 1 }),
        Format::Str => json!({ "type": "string" }),
        Format::Bytes => json!({
            "type": "array",
            "items": integer(u8::MIN, u8::MAX)
        }),
        Format::Option(inner) => choice(vec![schema(inner), json!({ "type": "null" })]),
        Format::Seq(inner) => json!({ "type": "array", "items": schema(inner) }),
        Format::Map { key, value } => json!({
            "type": "object",
            "propertyNames": schema(key),
            "additionalProperties": schema(value)
        }),
        Format::Tuple(formats) => tuple(formats),
        Format::TupleArray { content, size } => json!({
            "type": "array",
            "items": schema(content),
            "minItems": size,
            "maxItems": size
        }),
    }
}

fn variant(name: &str, format: &VariantFormat) -> Value {
    match format {
        VariantFormat::Variable(variable) => variable
            .borrow()
            .as_ref()
            .map_or_else(|| tagged(name, &json!({})), |format| variant(name, format)),
        VariantFormat::Unit => json!({ "const": name }),
        VariantFormat::NewType(format) => tagged(name, &schema(format)),
        VariantFormat::Tuple(formats) => tagged(name, &tuple(formats)),
        VariantFormat::Struct(fields) => tagged(name, &structure(fields)),
    }
}

fn structure(fields: &[Named<Format>]) -> Value {
    let properties = fields
        .iter()
        .map(|field| (field.name.clone(), schema(&field.value)))
        .collect::<Map<_, _>>();
    let required = fields.iter().map(|field| &field.name).collect::<Vec<_>>();
    json!({
        "type": "object",
        "properties": properties,
        "required": required,
        "additionalProperties": false
    })
}

fn tuple(formats: &[Format]) -> Value {
    json!({
        "type": "array",
        "prefixItems": formats.iter().map(schema).collect::<Vec<_>>(),
        "items": false,
        "minItems": formats.len(),
        "maxItems": formats.len()
    })
}

fn choice(options: Vec<Value>) -> Value {
    if options.len() == 1 {
        options.into_iter().next().expect("one option")
    } else {
        json!({ "anyOf": options })
    }
}

fn tagged(name: &str, value: &Value) -> Value {
    json!({
        "type": "object",
        "required": [name],
        "properties": { name: value },
        "additionalProperties": false
    })
}

fn integer<T: serde::Serialize>(minimum: T, maximum: T) -> Value {
    json!({ "type": "integer", "minimum": minimum, "maximum": maximum })
}

fn serde_key(name: &str) -> String {
    format!("serde/{name}")
}

fn value_key(name: &str) -> String {
    format!("value/{name}")
}

fn reference(key: &str) -> Value {
    crate::pointer::reference(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn traces_reflected_formats() {
        let formats = values().unwrap();
        assert!(!formats.variants.is_empty());
        assert!(formats
            .definitions
            .contains_key(&format!("serde/{}", formats.root)));
        assert!(formats
            .variants
            .iter()
            .all(|name| formats.definitions.contains_key(&format!("value/{name}"))));
    }
}

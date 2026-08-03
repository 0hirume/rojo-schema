use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

use anyhow::{bail, ensure, Context, Result};
use serde_json::{json, Map, Value};
use syn::{
    meta::ParseNestedMeta,
    visit::{self, Visit},
    AngleBracketedGenericArguments, Arm, Attribute, BinOp, Expr, ExprBinary, ExprCall, Fields,
    GenericArgument, ImplItem, Item, ItemEnum, ItemFn, ItemImpl, ItemStruct, ItemType, Lit, Meta,
    Pat, PathArguments, Stmt, Type, TypePath,
};

const PROJECT: &str = "Project";
const NODE: &str = "ProjectNode";

#[derive(Debug, Clone)]
pub struct Grammar {
    pub root: Value,
    pub definitions: Map<String, Value>,
    pub compact: BTreeMap<String, Value>,
    pub inferred: BTreeMap<String, Inference>,
    pub tree: String,
    pub node: Node,
}

#[derive(Debug, Clone, Default)]
pub struct Inference {
    pub names: BTreeSet<String>,
    pub tags: BTreeSet<String>,
}

#[derive(Debug, Clone)]
pub struct Node {
    pub schema: Value,
    pub class: String,
    pub properties: String,
    pub value: String,
}

#[derive(Default)]
struct Definitions {
    structs: BTreeMap<String, ItemStruct>,
    enums: BTreeMap<String, ItemEnum>,
    aliases: BTreeMap<String, ItemType>,
    strings: BTreeSet<String>,
    compact: BTreeMap<String, String>,
    inferred: BTreeMap<String, Inference>,
}

pub fn load(root: &Path) -> Result<Grammar> {
    let mut definitions = Definitions::default();
    for path in rust_files(root)? {
        let source = fs::read_to_string(&path)
            .with_context(|| format!("reading Rojo source {}", path.display()))?;
        let file = syn::parse_file(&source)
            .with_context(|| format!("parsing Rojo source {}", path.display()))?;
        collect_items(&file.items, &mut definitions)?;
    }
    ensure!(
        !definitions.inferred.is_empty(),
        "Rojo class inference rules were not found"
    );

    let tree = project_tree(&definitions)?;
    let roles = node_roles(&definitions)?;
    let mut builder = Builder::new(&definitions);
    let root_ref = builder.named(PROJECT)?;
    let root_key = definition_key(PROJECT);
    let root = builder.output.get(&root_key).cloned().unwrap_or(root_ref);
    let node_key = definition_key(NODE);
    let schema = builder
        .output
        .get(&node_key)
        .cloned()
        .context("ProjectNode was not reachable from Project")?;
    let compact = compact_schemas(&definitions, &mut builder)?;

    Ok(Grammar {
        root,
        definitions: builder.output,
        compact,
        inferred: definitions.inferred,
        tree,
        node: Node {
            schema,
            class: roles.class,
            properties: roles.properties,
            value: roles.value,
        },
    })
}

fn rust_files(root: &Path) -> Result<Vec<PathBuf>> {
    fn visit(current: &Path, output: &mut Vec<PathBuf>) -> Result<()> {
        let mut entries = fs::read_dir(current)
            .with_context(|| format!("reading {}", current.display()))?
            .map(|entry| entry.map(|entry| entry.path()))
            .collect::<std::io::Result<Vec<_>>>()?;
        entries.sort();
        for path in entries {
            if path.is_dir() {
                visit(&path, output)?;
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                output.push(path);
            }
        }
        Ok(())
    }

    let mut output = Vec::new();
    visit(root, &mut output)?;
    Ok(output)
}

fn collect_items(items: &[Item], definitions: &mut Definitions) -> Result<()> {
    for item in items {
        match item {
            Item::Struct(item) => {
                definitions
                    .structs
                    .entry(item.ident.to_string())
                    .or_insert_with(|| item.clone());
            }
            Item::Enum(item) => {
                definitions
                    .enums
                    .entry(item.ident.to_string())
                    .or_insert_with(|| item.clone());
            }
            Item::Type(item) => {
                definitions
                    .aliases
                    .entry(item.ident.to_string())
                    .or_insert_with(|| item.clone());
            }
            Item::Impl(item) => collect_impl(item, definitions)?,
            Item::Fn(item) => collect_inference(item, definitions)?,
            Item::Mod(item) => {
                if let Some((_, items)) = &item.content {
                    collect_items(items, definitions)?;
                }
            }
            _ => {}
        }
    }
    Ok(())
}

fn collect_inference(item: &ItemFn, definitions: &mut Definitions) -> Result<()> {
    if item.sig.ident != "infer_class_name" {
        return Ok(());
    }
    let mut branch = item
        .block
        .stmts
        .iter()
        .find_map(|statement| match statement {
            Stmt::Expr(Expr::If(branch), _)
                if comparison(&branch.cond, "parent_class").is_some() =>
            {
                Some(branch)
            }
            _ => None,
        });
    while let Some(current) = branch {
        let parent = comparison(&current.cond, "parent_class")
            .context("Rojo class inference branch has no parent class")?;
        let mut inference = InferenceVisitor::default();
        inference.visit_block(&current.then_branch);
        ensure!(
            !inference.names.is_empty() || !inference.tags.is_empty(),
            "Rojo class inference for {parent} has no names or tags"
        );
        let target = definitions.inferred.entry(parent).or_default();
        target.names.extend(inference.names);
        target.tags.extend(inference.tags);
        branch =
            current
                .else_branch
                .as_ref()
                .and_then(|(_, expression)| match expression.as_ref() {
                    Expr::If(branch) => Some(branch),
                    _ => None,
                });
    }
    Ok(())
}

#[derive(Default)]
struct InferenceVisitor {
    names: BTreeSet<String>,
    tags: BTreeSet<String>,
}

impl<'ast> Visit<'ast> for InferenceVisitor {
    fn visit_expr_binary(&mut self, node: &'ast ExprBinary) {
        if let Some(name) = binary_comparison(node, "name") {
            self.names.insert(name);
        }
        visit::visit_expr_binary(self, node);
    }

    fn visit_expr_path(&mut self, node: &'ast syn::ExprPath) {
        let mut segments = node.path.segments.iter().rev();
        if let (Some(tag), Some(owner)) = (segments.next(), segments.next()) {
            if owner.ident == "ClassTag" {
                self.tags.insert(tag.ident.to_string());
            }
        }
        visit::visit_expr_path(self, node);
    }
}

fn comparison(expression: &Expr, name: &str) -> Option<String> {
    let Expr::Binary(binary) = expression else {
        return None;
    };
    binary_comparison(binary, name)
}

fn binary_comparison(expression: &ExprBinary, name: &str) -> Option<String> {
    if !matches!(expression.op, BinOp::Eq(_)) {
        return None;
    }
    identifier_string(&expression.left, &expression.right, name)
        .or_else(|| identifier_string(&expression.right, &expression.left, name))
}

fn identifier_string(identifier: &Expr, value: &Expr, name: &str) -> Option<String> {
    let Expr::Path(identifier) = identifier else {
        return None;
    };
    let Expr::Lit(value) = value else {
        return None;
    };
    let Lit::Str(value) = &value.lit else {
        return None;
    };
    identifier
        .path
        .get_ident()
        .is_some_and(|identifier| identifier == name)
        .then(|| value.value())
}

fn collect_impl(item: &ItemImpl, definitions: &mut Definitions) -> Result<()> {
    let Some(name) = type_name(&item.self_ty) else {
        return Ok(());
    };
    if item.trait_.as_ref().is_some_and(|(trait_path, _)| {
        trait_path
            .segments
            .last()
            .is_some_and(|segment| segment.ident == "Deserialize")
    }) {
        let mut visitor = StringDeserializer::default();
        visitor.visit_item_impl(item);
        if visitor.found {
            definitions.strings.insert(name.clone());
        }
    }
    if name == "AmbiguousValue" {
        let mut visitor = CompactResolver::default();
        for member in &item.items {
            if let ImplItem::Fn(method) = member {
                if method.sig.ident == "resolve" {
                    visitor.visit_block(&method.block);
                }
            }
        }
        for (variant, ambiguous) in visitor.values {
            if let Some(previous) = definitions
                .compact
                .insert(variant.clone(), ambiguous.clone())
            {
                ensure!(
                    previous == ambiguous,
                    "Rojo resolves {variant} through both {previous} and {ambiguous}"
                );
            }
        }
    }
    Ok(())
}

#[derive(Default)]
struct StringDeserializer {
    found: bool,
}

impl<'ast> Visit<'ast> for StringDeserializer {
    fn visit_expr_call(&mut self, node: &'ast ExprCall) {
        if let Expr::Path(path) = node.func.as_ref() {
            let segments = &path.path.segments;
            if segments.len() >= 2
                && segments[segments.len() - 2].ident == "String"
                && segments
                    .last()
                    .is_some_and(|segment| segment.ident == "deserialize")
            {
                self.found = true;
            }
        }
        visit::visit_expr_call(self, node);
    }
}

#[derive(Default)]
struct CompactResolver {
    values: Vec<(String, String)>,
}

impl<'ast> Visit<'ast> for CompactResolver {
    fn visit_arm(&mut self, node: &'ast Arm) {
        if expression_calls(&node.body, "Ok") {
            if let Some(value) = resolution_pair(&node.pat) {
                self.values.push(value);
            }
        }
        visit::visit_arm(self, node);
    }
}

#[derive(Default)]
struct CallFinder<'a> {
    name: &'a str,
    found: bool,
}

impl<'ast> Visit<'ast> for CallFinder<'_> {
    fn visit_expr_call(&mut self, node: &'ast ExprCall) {
        if let Expr::Path(path) = node.func.as_ref() {
            if path
                .path
                .segments
                .last()
                .is_some_and(|segment| segment.ident == self.name)
            {
                self.found = true;
            }
        }
        visit::visit_expr_call(self, node);
    }
}

fn expression_calls(expression: &Expr, name: &str) -> bool {
    let mut visitor = CallFinder { name, found: false };
    visitor.visit_expr(expression);
    visitor.found
}

fn resolution_pair(pattern: &Pat) -> Option<(String, String)> {
    let Pat::Tuple(tuple) = pattern else {
        return None;
    };
    let mut patterns = tuple.elems.iter();
    let variant = pattern_variant(patterns.next()?, "VariantType")?;
    let ambiguous = pattern_variant(patterns.next()?, "AmbiguousValue")?;
    patterns.next().is_none().then_some((variant, ambiguous))
}

fn pattern_variant(pattern: &Pat, owner: &str) -> Option<String> {
    let path = match pattern {
        Pat::Path(pattern) => &pattern.path,
        Pat::TupleStruct(pattern) => &pattern.path,
        _ => return None,
    };
    let mut segments = path.segments.iter().rev();
    let variant = segments.next()?;
    let parent = segments.next()?;
    (parent.ident == owner).then(|| variant.ident.to_string())
}

struct Roles {
    class: String,
    properties: String,
    value: String,
}

fn project_tree(definitions: &Definitions) -> Result<String> {
    let project = definitions
        .structs
        .get(PROJECT)
        .context("Rojo Project struct was not found")?;
    let attributes = ContainerAttrs::parse(&project.attrs)?;
    for field in &project.fields {
        if base_name(&field.ty).as_deref() == Some(NODE) {
            return field_name(field, attributes.rename_all.as_deref());
        }
    }
    bail!("Rojo Project has no ProjectNode field")
}

fn node_roles(definitions: &Definitions) -> Result<Roles> {
    let node = definitions
        .structs
        .get(NODE)
        .context("Rojo ProjectNode struct was not found")?;
    let attributes = ContainerAttrs::parse(&node.attrs)?;
    let mut property_map = None;

    for field in &node.fields {
        let field_attributes = FieldAttrs::parse(&field.attrs)?;
        if field_attributes.skip || field_attributes.flatten {
            continue;
        }
        if let Some((key, value)) = map_types(&field.ty) {
            let Some(key) = base_name(key) else {
                continue;
            };
            if key != "String" {
                property_map = Some((
                    field_name(field, attributes.rename_all.as_deref())?,
                    key,
                    base_name(value).context("ProjectNode property value type not found")?,
                ));
            }
        }
    }

    let (properties, key, value) = property_map.context("ProjectNode property map not found")?;
    let class = node
        .fields
        .iter()
        .find(|field| base_name(&field.ty).as_deref() == Some(key.as_str()))
        .context("ProjectNode class field not found")?;
    let class = field_name(class, attributes.rename_all.as_deref())?;
    Ok(Roles {
        class,
        properties,
        value,
    })
}

fn compact_schemas(
    definitions: &Definitions,
    builder: &mut Builder<'_>,
) -> Result<BTreeMap<String, Value>> {
    ensure!(
        !definitions.compact.is_empty(),
        "Rojo compact value resolver mappings were not found"
    );
    let ambiguous = definitions
        .enums
        .get("AmbiguousValue")
        .context("Rojo AmbiguousValue enum was not found")?;
    let attributes = ContainerAttrs::parse(&ambiguous.attrs)?;
    definitions
        .compact
        .iter()
        .map(|(variant, name)| {
            let item = ambiguous
                .variants
                .iter()
                .find(|item| item.ident == name)
                .with_context(|| format!("Rojo AmbiguousValue::{name} was not found"))?;
            Ok((variant.clone(), builder.variant(item, &attributes)?))
        })
        .collect()
}

struct Builder<'a> {
    definitions: &'a Definitions,
    output: Map<String, Value>,
    building: BTreeSet<String>,
}

impl<'a> Builder<'a> {
    fn new(definitions: &'a Definitions) -> Self {
        Self {
            definitions,
            output: Map::new(),
            building: BTreeSet::new(),
        }
    }

    fn named(&mut self, name: &str) -> Result<Value> {
        if let Some(schema) = standard(name) {
            return Ok(schema);
        }
        if self.definitions.strings.contains(name) {
            return Ok(json!({ "type": "string" }));
        }
        let key = definition_key(name);
        if self.output.contains_key(&key) || self.building.contains(name) {
            return Ok(reference(&key));
        }

        self.building.insert(name.to_owned());
        let schema = if let Some(item) = self.definitions.structs.get(name) {
            self.structure(item)?
        } else if let Some(item) = self.definitions.enums.get(name) {
            self.enumeration(item)?
        } else if let Some(item) = self.definitions.aliases.get(name) {
            self.ty(&item.ty)?
        } else {
            json!({})
        };
        self.building.remove(name);
        self.output.insert(key.clone(), schema);
        Ok(reference(&key))
    }

    fn ty(&mut self, ty: &Type) -> Result<Value> {
        match ty {
            Type::Array(array) => {
                let length = match &array.len {
                    Expr::Lit(value) => match &value.lit {
                        Lit::Int(value) => value.base10_parse::<usize>()?,
                        _ => return Ok(json!({ "type": "array" })),
                    },
                    _ => return Ok(json!({ "type": "array" })),
                };
                let item = self.ty(&array.elem)?;
                Ok(json!({
                    "type": "array",
                    "items": item,
                    "minItems": length,
                    "maxItems": length
                }))
            }
            Type::Reference(reference) => self.ty(&reference.elem),
            Type::Slice(slice) => Ok(json!({ "type": "array", "items": self.ty(&slice.elem)? })),
            Type::Tuple(tuple) => {
                let items = tuple
                    .elems
                    .iter()
                    .map(|item| self.ty(item))
                    .collect::<Result<Vec<_>>>()?;
                Ok(tuple_schema(&items))
            }
            Type::Path(path) => self.path(path),
            _ => Ok(json!({})),
        }
    }

    fn path(&mut self, path: &TypePath) -> Result<Value> {
        let Some(segment) = path.path.segments.last() else {
            return Ok(json!({}));
        };
        let name = segment.ident.to_string();
        let arguments = type_arguments(&segment.arguments);
        match name.as_str() {
            "Option" => {
                let Some(inner) = arguments.first() else {
                    return Ok(json!({}));
                };
                Ok(json!({ "anyOf": [self.ty(inner)?, { "type": "null" }] }))
            }
            "Vec" | "VecDeque" => {
                let Some(inner) = arguments.first() else {
                    return Ok(json!({ "type": "array" }));
                };
                Ok(json!({ "type": "array", "items": self.ty(inner)? }))
            }
            "HashSet" | "BTreeSet" => {
                let Some(inner) = arguments.first() else {
                    return Ok(json!({ "type": "array", "uniqueItems": true }));
                };
                Ok(json!({
                    "type": "array",
                    "items": self.ty(inner)?,
                    "uniqueItems": true
                }))
            }
            "HashMap" | "BTreeMap" | "IndexMap" => {
                if arguments.len() < 2 {
                    return Ok(json!({ "type": "object" }));
                }
                Ok(json!({
                    "type": "object",
                    "propertyNames": self.ty(arguments[0])?,
                    "additionalProperties": self.ty(arguments[1])?
                }))
            }
            "Box" | "Arc" | "Rc" | "Cow" => arguments
                .last()
                .map_or_else(|| Ok(json!({})), |inner| self.ty(inner)),
            _ => self.named(&name),
        }
    }

    fn structure(&mut self, item: &ItemStruct) -> Result<Value> {
        let attributes = ContainerAttrs::parse(&item.attrs)?;
        let mut schema = match &item.fields {
            Fields::Named(fields) => {
                let mut properties = Map::new();
                let mut required = Vec::new();
                let mut additional = if attributes.deny_unknown {
                    Value::Bool(false)
                } else {
                    Value::Object(Map::new())
                };
                let mut flattened = Vec::new();
                for field in &fields.named {
                    let field_attributes = FieldAttrs::parse(&field.attrs)?;
                    if field_attributes.skip {
                        continue;
                    }
                    if field_attributes.flatten {
                        if let Some((_, value)) = map_types(&field.ty) {
                            additional = self.ty(value)?;
                        } else {
                            flattened.push(self.ty(&field.ty)?);
                        }
                        continue;
                    }
                    let name = field_name(field, attributes.rename_all.as_deref())?;
                    let mut value = self.ty(&field.ty)?;
                    describe(&mut value, &field.attrs);
                    if !is_option(&field.ty) && !field_attributes.default {
                        required.push(name.clone());
                    }
                    properties.insert(name, value);
                }
                let mut object = json!({
                    "type": "object",
                    "properties": properties,
                    "additionalProperties": additional
                });
                if !required.is_empty() {
                    object["required"] = json!(required);
                }
                if flattened.is_empty() {
                    object
                } else {
                    flattened.insert(0, object);
                    json!({ "allOf": flattened })
                }
            }
            Fields::Unnamed(fields) if fields.unnamed.len() == 1 => {
                self.ty(&fields.unnamed[0].ty)?
            }
            Fields::Unnamed(fields) => {
                let items = fields
                    .unnamed
                    .iter()
                    .map(|field| self.ty(&field.ty))
                    .collect::<Result<Vec<_>>>()?;
                tuple_schema(&items)
            }
            Fields::Unit => json!({ "type": "null" }),
        };
        describe(&mut schema, &item.attrs);
        Ok(schema)
    }

    fn enumeration(&mut self, item: &ItemEnum) -> Result<Value> {
        let attributes = ContainerAttrs::parse(&item.attrs)?;
        let mut variants = Vec::new();
        for variant in &item.variants {
            let variant_attributes = FieldAttrs::parse(&variant.attrs)?;
            if variant_attributes.skip {
                continue;
            }
            variants.push(self.variant(variant, &attributes)?);
        }
        let mut schema = if variants.len() == 1 {
            variants.pop().expect("one variant")
        } else {
            json!({ "anyOf": variants })
        };
        describe(&mut schema, &item.attrs);
        Ok(schema)
    }

    fn variant(&mut self, variant: &syn::Variant, attributes: &ContainerAttrs) -> Result<Value> {
        let variant_attributes = FieldAttrs::parse(&variant.attrs)?;
        let name = variant_attributes.rename.unwrap_or_else(|| {
            rename(&variant.ident.to_string(), attributes.rename_all.as_deref())
        });
        match &variant.fields {
            Fields::Unit => Ok(json!({ "const": name })),
            Fields::Unnamed(fields) if fields.unnamed.len() == 1 => {
                let inner = self.ty(&fields.unnamed[0].ty)?;
                Ok(if attributes.untagged {
                    inner
                } else {
                    tagged(&name, inner)
                })
            }
            Fields::Unnamed(fields) => {
                let items = fields
                    .unnamed
                    .iter()
                    .map(|field| self.ty(&field.ty))
                    .collect::<Result<Vec<_>>>()?;
                let inner = tuple_schema(&items);
                Ok(if attributes.untagged {
                    inner
                } else {
                    tagged(&name, inner)
                })
            }
            Fields::Named(fields) => {
                let mut properties = Map::new();
                let mut required = Vec::new();
                for field in &fields.named {
                    let field_attributes = FieldAttrs::parse(&field.attrs)?;
                    if field_attributes.skip {
                        continue;
                    }
                    let field_name = field_name(field, attributes.rename_all.as_deref())?;
                    if !is_option(&field.ty) && !field_attributes.default {
                        required.push(field_name.clone());
                    }
                    properties.insert(field_name, self.ty(&field.ty)?);
                }
                let mut inner = json!({
                    "type": "object",
                    "properties": properties,
                    "additionalProperties": false
                });
                if !required.is_empty() {
                    inner["required"] = json!(required);
                }
                Ok(if attributes.untagged {
                    inner
                } else {
                    tagged(&name, inner)
                })
            }
        }
    }
}

#[derive(Default)]
struct ContainerAttrs {
    rename_all: Option<String>,
    deny_unknown: bool,
    untagged: bool,
}

impl ContainerAttrs {
    fn parse(attributes: &[Attribute]) -> Result<Self> {
        let mut output = Self::default();
        for attribute in attributes {
            if !attribute.path().is_ident("serde") {
                continue;
            }
            attribute.parse_nested_meta(|meta| {
                if meta.path.is_ident("rename_all") {
                    output.rename_all = Some(meta.value()?.parse::<syn::LitStr>()?.value());
                } else if meta.path.is_ident("deny_unknown_fields") {
                    output.deny_unknown = true;
                } else if meta.path.is_ident("untagged") {
                    output.untagged = true;
                } else {
                    discard_meta(&meta)?;
                }
                Ok(())
            })?;
        }
        Ok(output)
    }
}

#[derive(Default)]
struct FieldAttrs {
    rename: Option<String>,
    default: bool,
    flatten: bool,
    skip: bool,
}

impl FieldAttrs {
    fn parse(attributes: &[Attribute]) -> Result<Self> {
        let mut output = Self::default();
        for attribute in attributes {
            if !attribute.path().is_ident("serde") {
                continue;
            }
            attribute.parse_nested_meta(|meta| {
                if meta.path.is_ident("rename") {
                    output.rename = Some(meta.value()?.parse::<syn::LitStr>()?.value());
                } else if meta.path.is_ident("default") {
                    output.default = true;
                    discard_meta(&meta)?;
                } else if meta.path.is_ident("flatten") {
                    output.flatten = true;
                } else if meta.path.is_ident("skip") || meta.path.is_ident("skip_deserializing") {
                    output.skip = true;
                } else {
                    discard_meta(&meta)?;
                }
                Ok(())
            })?;
        }
        Ok(output)
    }
}

fn discard_meta(meta: &ParseNestedMeta<'_>) -> syn::Result<()> {
    if meta.input.peek(syn::Token![=]) {
        let _: Expr = meta.value()?.parse()?;
    } else if meta.input.peek(syn::token::Paren) {
        meta.parse_nested_meta(|nested| discard_meta(&nested))?;
    }
    Ok(())
}

fn field_name(field: &syn::Field, rename_all: Option<&str>) -> Result<String> {
    let attributes = FieldAttrs::parse(&field.attrs)?;
    if let Some(rename) = attributes.rename {
        return Ok(rename);
    }
    let name = field.ident.as_ref().context("unnamed field")?.to_string();
    Ok(rename(&name, rename_all))
}

fn rename(name: &str, rule: Option<&str>) -> String {
    match rule {
        Some("camelCase") => {
            let mut output = String::new();
            let mut upper = false;
            for character in name.chars() {
                if character == '_' || character == '-' {
                    upper = true;
                } else if output.is_empty() {
                    output.push(character.to_ascii_lowercase());
                } else if upper {
                    output.push(character.to_ascii_uppercase());
                    upper = false;
                } else {
                    output.push(character);
                }
            }
            output
        }
        Some("kebab-case") => name.replace('_', "-").to_ascii_lowercase(),
        Some("snake_case" | "lowercase") => name.to_ascii_lowercase(),
        Some("UPPERCASE" | "SCREAMING_SNAKE_CASE") => name.to_ascii_uppercase(),
        _ => name.to_owned(),
    }
}

fn type_arguments(arguments: &PathArguments) -> Vec<&Type> {
    let PathArguments::AngleBracketed(AngleBracketedGenericArguments { args, .. }) = arguments
    else {
        return Vec::new();
    };
    args.iter()
        .filter_map(|argument| match argument {
            GenericArgument::Type(ty) => Some(ty),
            _ => None,
        })
        .collect()
}

fn type_name(ty: &Type) -> Option<String> {
    let Type::Path(path) = ty else {
        return None;
    };
    path.path
        .segments
        .last()
        .map(|segment| segment.ident.to_string())
}

fn base_name(ty: &Type) -> Option<String> {
    let Type::Path(path) = ty else {
        return None;
    };
    let segment = path.path.segments.last()?;
    if segment.ident == "Option" {
        return type_arguments(&segment.arguments)
            .first()
            .and_then(|inner| base_name(inner));
    }
    Some(segment.ident.to_string())
}

fn map_types(ty: &Type) -> Option<(&Type, &Type)> {
    let Type::Path(path) = ty else {
        return None;
    };
    let segment = path.path.segments.last()?;
    if segment.ident == "Option" {
        return type_arguments(&segment.arguments)
            .first()
            .and_then(|inner| map_types(inner));
    }
    if !matches!(
        segment.ident.to_string().as_str(),
        "BTreeMap" | "HashMap" | "IndexMap"
    ) {
        return None;
    }
    let arguments = type_arguments(&segment.arguments);
    (arguments.len() >= 2).then(|| (arguments[0], arguments[1]))
}

fn is_option(ty: &Type) -> bool {
    matches!(ty, Type::Path(path) if path.path.segments.last().is_some_and(|segment| segment.ident == "Option"))
}

fn standard(name: &str) -> Option<Value> {
    match name {
        "bool" => Some(json!({ "type": "boolean" })),
        "i8" => Some(integer(i8::MIN, i8::MAX)),
        "i16" => Some(integer(i16::MIN, i16::MAX)),
        "i32" => Some(integer(i32::MIN, i32::MAX)),
        "i64" => Some(integer(i64::MIN, i64::MAX)),
        "u8" => Some(integer(u8::MIN, u8::MAX)),
        "u16" => Some(integer(u16::MIN, u16::MAX)),
        "u32" => Some(integer(u32::MIN, u32::MAX)),
        "u64" => Some(integer(u64::MIN, u64::MAX)),
        "f32" | "f64" => Some(json!({ "type": "number" })),
        "str" | "String" | "Ustr" | "Path" | "PathBuf" | "OsStr" | "OsString" => {
            Some(json!({ "type": "string" }))
        }
        "IpAddr" => Some(json!({
            "type": "string",
            "anyOf": [{ "format": "ipv4" }, { "format": "ipv6" }]
        })),
        "Value" => Some(json!({})),
        _ => None,
    }
}

fn integer<T: serde::Serialize>(minimum: T, maximum: T) -> Value {
    json!({ "type": "integer", "minimum": minimum, "maximum": maximum })
}

fn tagged(name: &str, value: Value) -> Value {
    let mut properties = Map::new();
    properties.insert(name.to_owned(), value);
    json!({
        "type": "object",
        "required": [name],
        "properties": properties,
        "additionalProperties": false
    })
}

fn tuple_schema(items: &[Value]) -> Value {
    let length = items.len();
    json!({
        "type": "array",
        "prefixItems": items,
        "items": false,
        "minItems": length,
        "maxItems": length
    })
}

fn describe(schema: &mut Value, attributes: &[Attribute]) {
    let description = attributes
        .iter()
        .filter_map(|attribute| {
            if !attribute.path().is_ident("doc") {
                return None;
            }
            let Meta::NameValue(value) = &attribute.meta else {
                return None;
            };
            let Expr::Lit(value) = &value.value else {
                return None;
            };
            let Lit::Str(value) = &value.lit else {
                return None;
            };
            Some(value.value().trim().to_owned())
        })
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    if !description.is_empty() {
        schema
            .as_object_mut()
            .expect("schema object")
            .insert("description".to_owned(), json!(description));
    }
}

fn definition_key(name: &str) -> String {
    format!("rojo/{name}")
}

fn reference(key: &str) -> Value {
    crate::pointer::reference(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_rojo_project_grammar() {
        let grammar = load(Path::new("../../../../src/rojo/src")).unwrap();
        assert_eq!(grammar.root["required"], json!(["tree"]));
        assert!(grammar.definitions.contains_key("rojo/ProjectNode"));
        assert_eq!(grammar.compact["Bool"], json!({ "type": "boolean" }));
        assert_eq!(grammar.compact["CFrame"]["type"], "array");
        assert!(!grammar.compact.contains_key("Ref"));
        assert!(grammar.inferred["DataModel"].tags.contains("Service"));
        assert!(grammar.inferred["StarterPlayer"]
            .names
            .contains("StarterPlayerScripts"));
        assert!(grammar.inferred["Workspace"].names.contains("Terrain"));
        assert_eq!(grammar.tree, "tree");
    }
}

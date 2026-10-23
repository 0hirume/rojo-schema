use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

use eyre::{Context as _, ContextCompat as _, Result, ensure};
use serde_json::{Map, Value, json};

use syn::{
    Arm, Expr, ExprMacro, ExprTuple, Item, Lit, Macro, Pat, Token,
    parse::Parser as _,
    punctuated::Punctuated,
    visit::{self, Visit},
};

use super::{grammar::Grammar, pointer};

#[derive(Debug, Clone)]
pub(super) struct Rule {
    pub pattern: String,
    pub exclude: Option<String>,
    pub classes: BTreeSet<String>,
}

#[derive(Default)]
struct Vector(Option<Macro>);

impl<'source> Visit<'source> for Vector {
    fn visit_expr_macro(&mut self, expression: &'source ExprMacro) {
        if expression.mac.path.is_ident("vec") {
            self.0 = Some(expression.mac.clone());
        }
    }
}

#[derive(Default)]
struct Classes(BTreeSet<String>);

impl<'source> Visit<'source> for Classes {
    fn visit_expr_tuple(&mut self, expression: &'source ExprTuple) {
        if let Some(Expr::Lit(value)) = expression.elems.first()
            && let Lit::Str(value) = &value.lit
        {
            self.0.insert(value.value());
        }

        visit::visit_expr_tuple(self, expression);
    }
}

#[derive(Default)]
struct Scripts(BTreeMap<String, BTreeSet<String>>);

impl<'source> Visit<'source> for Scripts {
    fn visit_arm(&mut self, arm: &'source Arm) {
        if let Pat::Path(pattern) = &arm.pat
            && pattern
                .path
                .segments
                .first()
                .is_some_and(|part| part.ident == "ScriptType")
            && let Some(name) = pattern.path.segments.last()
        {
            let mut classes = Classes::default();
            classes.visit_expr(&arm.body);
            self.0.insert(name.ident.to_string(), classes.0);
        }

        visit::visit_arm(self, arm);
    }
}

pub(super) fn load(root: &Path) -> Result<Vec<Rule>> {
    let module = source(&root.join("snapshot_middleware/mod.rs"))?;
    let scripts = source(&root.join("snapshot_middleware/lua.rs"))?;

    let function = module
        .items
        .iter()
        .find_map(|item| match item {
            Item::Fn(function) if function.sig.ident == "default_sync_rules" => Some(function),
            _ => None,
        })
        .context("Rojo default sync rules were not found")?;

    let mut vector = Vector::default();
    vector.visit_block(&function.block);

    let vector = vector
        .0
        .context("Rojo default sync rule list was not found")?;

    let expressions = Punctuated::<Expr, Token![,]>::parse_terminated.parse2(vector.tokens)?;
    let mut classes = Scripts::default();

    for item in &scripts.items {
        if let Item::Fn(function) = item
            && function.sig.ident == "snapshot_lua"
        {
            classes.visit_block(&function.block);
        }
    }

    ensure!(
        !classes.0.is_empty(),
        "Rojo script class mappings were not found"
    );

    let mut rules = Vec::new();

    for expression in expressions {
        let Expr::Macro(expression) = expression else {
            eyre::bail!("unsupported Rojo default sync rule expression");
        };

        ensure!(
            expression.mac.path.is_ident("sync_rule"),
            "unsupported Rojo sync rule macro"
        );

        let arguments =
            Punctuated::<Expr, Token![,]>::parse_terminated.parse2(expression.mac.tokens)?;

        ensure!(
            (2..=4).contains(&arguments.len()),
            "unsupported Rojo sync rule arguments"
        );

        let pattern = literal(&arguments[0]).context("sync rule pattern is not a string")?;

        ensure!(
            !pattern.contains(['[', ']', '\\']),
            "unsupported Rojo sync rule glob: {pattern}"
        );

        let Expr::Path(middleware) = &arguments[1] else {
            eyre::bail!("sync rule middleware is not a name");
        };

        let middleware = middleware
            .path
            .get_ident()
            .context("sync rule middleware is not a simple name")?
            .to_string();

        let script = middleware.strip_suffix("Script");

        let selected = script
            .and_then(|name| classes.0.get(name))
            .cloned()
            .unwrap_or_default();

        let exclude = arguments
            .iter()
            .nth(3)
            .map(|expression| literal(expression).context("sync rule exclusion is not a string"))
            .transpose()?;

        rules.push(Rule {
            pattern,
            exclude,
            classes: selected,
        });
    }

    Ok(rules)
}

fn source(path: &Path) -> Result<syn::File> {
    let text = fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;

    syn::parse_file(&text).with_context(|| format!("parsing {}", path.display()))
}

fn literal(expression: &Expr) -> Option<String> {
    if let Expr::Lit(value) = expression
        && let Lit::Str(value) = &value.lit
    {
        Some(value.value())
    } else {
        None
    }
}

pub(super) fn pattern(glob: &str) -> String {
    let mut pattern = String::from("(^|[/\\\\])");

    for character in glob.chars() {
        match character {
            '*' => pattern.push_str("[^/\\\\]*"),
            '?' => pattern.push_str("[^/\\\\]"),
            '{' => pattern.push_str("(?:"),
            '}' => pattern.push(')'),
            ',' => pattern.push('|'),

            '.' | '+' | '(' | ')' | '[' | ']' | '^' | '$' | '|' | '\\' => {
                pattern.push('\\');
                pattern.push(character);
            }

            _ => pattern.push(character),
        }
    }

    pattern.push('$');

    pattern
}

pub(super) fn apply(project: &mut Value, grammar: &Grammar) -> Result<()> {
    let definitions = project
        .get_mut("$defs")
        .and_then(Value::as_object_mut)
        .context("project definitions are missing")?;

    let custom = definitions
        .iter()
        .filter(|(name, _)| name.starts_with("node/") || name.starts_with("inferred/"))
        .map(|(name, schema)| {
            let mut schema = schema.clone();
            custom_references(&mut schema);

            (format!("custom/{name}"), schema)
        })
        .collect::<Map<_, _>>();

    definitions.extend(custom);

    let fallback = definitions["node/Any"]["allOf"]
        .as_array()
        .and_then(|rules| rules.iter().find(|rule| rule.get("if").is_some()))
        .and_then(|rule| rule.get("else"))
        .context("node class dispatch is missing")?
        .clone();

    let mut dispatch = fallback.clone();

    for rule in grammar.path_rules.iter().rev() {
        let mut condition = json!({ "type": "string", "pattern": pattern(&rule.pattern) });

        if let Some(exclude) = &rule.exclude {
            condition["not"] = json!({ "pattern": pattern(exclude) });
        }

        let hint = if rule.classes.is_empty() {
            fallback.clone()
        } else {
            path_hint(definitions, grammar, rule)?
        };

        dispatch = json!({
            "if": { "properties": { "$path": { "anyOf": [
                condition.clone(),
                { "type": "object", "properties": { "optional": condition }, "required": ["optional"] }
            ] } }, "required": ["$path"] },
            "then": hint,
            "else": dispatch
        });
    }

    let selection = definitions
        .get_mut("node/Any")
        .and_then(|node| node.get_mut("allOf"))
        .and_then(Value::as_array_mut)
        .and_then(|rules| rules.iter_mut().find(|rule| rule.get("if").is_some()))
        .context("node class dispatch is missing")?;

    selection["else"] = dispatch;

    let tree = project
        .get_mut("properties")
        .and_then(|fields| fields.get_mut(&grammar.tree))
        .and_then(Value::as_object_mut)
        .context("project tree is missing")?;

    tree.remove("$ref");

    project
        .as_object_mut()
        .context("project is not an object")?
        .entry("allOf")
        .or_insert_with(|| json!([]))
        .as_array_mut()
        .context("project allOf must be an array")?
        .push(json!({
            "if": { "properties": { "syncRules": { "maxItems": 0 } } },
            "then": { "properties": { grammar.tree.clone(): pointer::reference("node/Any") } },
            "else": { "properties": { grammar.tree.clone(): pointer::reference("custom/node/Any") } }
        }));

    Ok(())
}

fn path_hint(definitions: &Map<String, Value>, grammar: &Grammar, rule: &Rule) -> Result<Value> {
    let properties = rule
        .classes
        .iter()
        .map(|class| {
            let name = format!("properties/{class}");

            ensure!(
                definitions.contains_key(&name),
                "inferred class {class} has no property schema"
            );

            Ok(pointer::reference(&name))
        })
        .collect::<Result<Vec<_>>>()?;

    let classes = rule.classes.iter().cloned().collect::<Vec<_>>().join(" | ");

    Ok(json!({
        "title": classes,
        "x-rojo-class": classes,
        "markdownDescription": format!("Inferred from Rojo's `{}` file rule.", rule.pattern),
        "properties": { grammar.node.properties.clone(): {
            "markdownDescription": format!("Property suggestions for `{classes}`."),
            "anyOf": properties
        } }
    }))
}

fn custom_references(schema: &mut Value) {
    match schema {
        Value::Object(fields) => {
            if let Some(Value::String(reference)) = fields.get_mut("$ref")
                && (reference.starts_with("#/$defs/node~1")
                    || reference.starts_with("#/$defs/inferred~1"))
            {
                reference.insert_str("#/$defs/".len(), "custom~1");
            }

            for value in fields.values_mut() {
                custom_references(value);
            }
        }

        Value::Array(values) => {
            for value in values {
                custom_references(value);
            }
        }

        _ => {}
    }
}

use std::fmt::Write as _;

use eyre::{Context as _, Result};
use htmd::HtmlToMarkdown;
use pulldown_cmark::{Event, LinkType, Options, Parser, Tag, TagEnd, html::push_html};
use pulldown_cmark_to_cmark::cmark;
use serde_json::Value;
use url::Url;

const OPTIONS: Options = Options::ENABLE_TABLES
    .union(Options::ENABLE_STRIKETHROUGH)
    .union(Options::ENABLE_TASKLISTS);

pub(super) fn markdown(text: &str, base: &Url) -> Result<String> {
    let mut html = String::new();
    push_html(&mut html, Parser::new_ext(text, OPTIONS));

    let converted = HtmlToMarkdown::builder()
        .skip_tags(vec!["script", "style"])
        .build()
        .convert(&html)
        .context("converting documentation HTML")?;

    let mut events = Vec::new();
    let mut links = Vec::new();

    for event in Parser::new_ext(&converted, OPTIONS) {
        match event {
            Event::Start(mut tag) => {
                if let Tag::Link { dest_url, .. } | Tag::Image { dest_url, .. } = &mut tag {
                    if let Some(address) = destination(dest_url, base) {
                        *dest_url = address.into();
                        links.push(true);
                    } else {
                        links.push(false);
                        continue;
                    }
                }

                events.push(Event::Start(tag));
            }

            Event::End(TagEnd::Link | TagEnd::Image) => {
                if links.pop() == Some(true) {
                    events.push(event);
                }
            }

            Event::Code(text) => {
                if let Some((label, address)) = reference(&text) {
                    if let Some(address) = address {
                        events.push(Event::Start(Tag::Link {
                            link_type: LinkType::Inline,
                            dest_url: address.into(),
                            title: "".into(),
                            id: "".into(),
                        }));

                        events.push(Event::Code(label.into()));
                        events.push(Event::End(TagEnd::Link));
                    } else {
                        events.push(Event::Code(label.into()));
                    }
                } else {
                    events.push(Event::Code(text));
                }
            }

            Event::Html(_) | Event::InlineHtml(_) => {}
            _ => events.push(event),
        }
    }

    let mut output = String::new();
    cmark(events.iter(), &mut output, OPTIONS).context("formatting documentation Markdown")?;

    Ok(output.trim().to_owned())
}

fn destination(address: &str, base: &Url) -> Option<String> {
    if address.is_empty() {
        return None;
    }

    let mut address = base.join(address).ok()?;

    if !matches!(address.scheme(), "http" | "https") {
        return None;
    }

    if let Some(path) = address.path().strip_suffix(".md") {
        let path = path.to_owned();
        address.set_path(&path);
    }

    Some(address.into())
}

fn reference(text: &str) -> Option<(String, Option<String>)> {
    let (reference, label) = text
        .split_once('|')
        .map_or((text, None), |(reference, label)| (reference, Some(label)));

    let (kind, name) = reference.split_once('.')?;

    let directory = match kind {
        "Class" => "classes",
        "Datatype" => "datatypes",
        "Enum" => "enums",
        "Library" => "libraries",
        "Global" => "globals",
        _ => return None,
    };

    let display = label
        .filter(|label| *label != "no-link")
        .unwrap_or(name)
        .to_owned();

    if label == Some("no-link") {
        return Some((display, None));
    }

    let (owner, member) = name
        .split_once('.')
        .map_or((name, None), |(owner, member)| (owner, Some(member)));

    let mut address =
        format!("https://create.roblox.com/docs/reference/engine/{directory}/{owner}");

    if let Some(member) = member {
        address.push('#');
        address.push_str(member.trim_end_matches("()"));
    }

    Some((display, Some(address)))
}

pub(super) fn plaintext(text: &str) -> String {
    let mut output = String::new();
    let mut lists = Vec::new();
    let mut links = Vec::new();

    for event in Parser::new_ext(text, OPTIONS) {
        match event {
            Event::Text(text) | Event::Code(text) => output.push_str(&text),
            Event::SoftBreak | Event::HardBreak => output.push('\n'),
            Event::Start(Tag::List(start)) => lists.push(start),

            Event::End(TagEnd::List(_)) => {
                lists.pop();
            }

            Event::Start(Tag::Item) => {
                newline(&mut output);
                output.push_str(&"  ".repeat(lists.len().saturating_sub(1)));

                if let Some(Some(number)) = lists.last_mut() {
                    write!(output, "{number}. ").expect("writing to a string");
                    *number = number.saturating_add(1);
                } else {
                    output.push_str("- ");
                }
            }

            Event::End(TagEnd::Item | TagEnd::TableHead | TagEnd::TableRow) => newline(&mut output),
            Event::End(TagEnd::TableCell) => output.push('\t'),

            Event::Start(Tag::Link { dest_url, .. } | Tag::Image { dest_url, .. }) => {
                links.push((output.len(), dest_url));
            }

            Event::End(TagEnd::Link | TagEnd::Image) => {
                if let Some((start, destination)) = links.pop() {
                    let label = &output[start..];

                    if label.is_empty() {
                        output.push_str(&destination);
                    } else if label != destination.as_ref() {
                        write!(output, " ({destination})").expect("writing to a string");
                    }
                }
            }

            Event::TaskListMarker(checked) => {
                output.push_str(if checked { "[x] " } else { "[ ] " });
            }

            Event::End(
                TagEnd::Paragraph
                | TagEnd::Heading(_)
                | TagEnd::BlockQuote(_)
                | TagEnd::CodeBlock
                | TagEnd::Table,
            )
            | Event::Rule => {
                newline(&mut output);

                if !output.ends_with("\n\n") {
                    output.push('\n');
                }
            }

            _ => {}
        }
    }

    output.trim().to_owned()
}

fn newline(output: &mut String) {
    while output.ends_with('\t') {
        output.pop();
    }

    if !output.is_empty() && !output.ends_with('\n') {
        output.push('\n');
    }
}

pub(super) fn schema(schema: &mut Value, name: &str) {
    let kind = schema_type(schema);

    let Some(object) = schema.as_object_mut() else {
        return;
    };

    let title = object.remove("title");
    let name = title.as_ref().and_then(Value::as_str).unwrap_or(name);

    if let Some(description) = object
        .get("markdownDescription")
        .or_else(|| object.get("description"))
        .and_then(Value::as_str)
        .or_else(|| title.as_ref().map(|_| ""))
    {
        let mut label = String::new();
        cmark([Event::Text(name.into())], &mut label, OPTIONS).expect("formatting a string");

        let mut markdown = if name == kind {
            format!("**{label}**")
        } else {
            format!("**{label}: {kind}**")
        };

        markdown.push_str(&badges(object));

        if !description.is_empty() {
            write!(markdown, "\n\n{description}").expect("formatting a string");
        }

        if let Some(default) = object.get("default") {
            if default.is_array() || default.is_object() {
                write!(
                    markdown,
                    "\n\n**Default**\n\n```json\n{}\n```",
                    serde_json::to_string_pretty(default).expect("serializing a JSON value")
                )
                .expect("formatting a string");
            } else {
                markdown.push_str("\n\n**Default:** ");

                cmark(
                    [Event::Code(default.to_string().into())],
                    &mut markdown,
                    OPTIONS,
                )
                .expect("formatting a string");
            }
        }

        if object.get("deprecated").and_then(Value::as_bool) == Some(true)
            && let Some(message) = object.get("deprecationMessage").and_then(Value::as_str)
        {
            markdown.push_str("\n\n");

            cmark([Event::Text(message.into())], &mut markdown, OPTIONS)
                .expect("formatting a string");
        }

        object.insert(
            "description".to_owned(),
            Value::String(plaintext(&markdown)),
        );

        object.insert("markdownDescription".to_owned(), Value::String(markdown));
    }

    for keyword in ["properties", "patternProperties", "$defs", "definitions"] {
        if let Some(fields) = object.get_mut(keyword).and_then(Value::as_object_mut) {
            for (key, value) in fields {
                let label = if matches!(keyword, "$defs" | "definitions") {
                    match key.split_once('/') {
                        Some(("properties", _)) => "properties",
                        Some(("node" | "model", "Any")) => "instance",
                        _ => key.rsplit('/').next().unwrap_or(key),
                    }
                } else {
                    key
                };

                self::schema(value, label);
            }
        }
    }

    for keyword in ["anyOf", "oneOf", "allOf", "prefixItems"] {
        if let Some(options) = object.get_mut(keyword).and_then(Value::as_array_mut) {
            for option in options {
                self::schema(option, name);
            }
        }
    }

    for keyword in [
        "items",
        "additionalProperties",
        "unevaluatedProperties",
        "propertyNames",
        "contains",
        "not",
        "if",
        "then",
        "else",
    ] {
        if let Some(value) = object.get_mut(keyword) {
            self::schema(value, name);
        }
    }
}

fn schema_type(schema: &Value) -> String {
    if let Some(class) = schema.get("x-rojo-class").and_then(Value::as_str) {
        return class.to_owned();
    }

    if let Some(value_type) = schema.get("x-rojo-value-type").and_then(Value::as_str) {
        return match value_type {
            "Bool" => "boolean",
            "Float32" | "Float64" | "Int32" | "Int64" => "number",
            "String" => "string",
            name => name,
        }
        .to_owned();
    }

    if let Some(value) = schema.get("const") {
        return value.to_string();
    }

    if let Some(reference) = schema.get("$ref").and_then(Value::as_str) {
        let key = reference
            .strip_prefix("#/$defs/")
            .unwrap_or(reference)
            .replace("~1", "/")
            .replace("~0", "~");

        if key.starts_with("properties/") || key.starts_with("node/") || key.starts_with("model/") {
            return "object".to_owned();
        }

        return key.rsplit('/').next().unwrap_or(&key).to_owned();
    }

    if let Some(kind) = schema.get("type").and_then(Value::as_str) {
        return match kind {
            "integer" => "number".to_owned(),

            "array" => schema
                .get("prefixItems")
                .and_then(Value::as_array)
                .map_or_else(
                    || {
                        format!(
                            "Array<{}>",
                            schema
                                .get("items")
                                .map_or_else(|| "unknown".to_owned(), schema_type)
                        )
                    },
                    |items| {
                        format!(
                            "[{}]",
                            items.iter().map(schema_type).collect::<Vec<_>>().join(", ")
                        )
                    },
                ),

            _ => kind.to_owned(),
        };
    }

    if let Some(options) = schema
        .get("anyOf")
        .or_else(|| schema.get("oneOf"))
        .and_then(Value::as_array)
    {
        let mut kinds = Vec::new();

        for option in options {
            let kind = schema_type(option);

            if !kinds.contains(&kind) {
                kinds.push(kind);
            }
        }

        if !kinds.is_empty() {
            return kinds.join(" | ");
        }
    }

    "unknown".to_owned()
}

fn badges(object: &serde_json::Map<String, Value>) -> String {
    let mut markdown = String::new();

    match object.get("x-rojo-scriptability").and_then(Value::as_str) {
        Some("ReadWrite") => markdown.push_str(" · Writable"),
        Some("Read") => markdown.push_str(" · Read-only"),
        Some("None") => markdown.push_str(" · Non-scriptable"),
        _ => {}
    }

    if let Some(security) = object.get("x-roblox-security") {
        if let Some(fields) = security.as_object() {
            let read = fields.get("read").or_else(|| fields.get("Read"));
            let write = fields.get("write").or_else(|| fields.get("Write"));
            let combined = read.is_some() && read == write;

            for (access, restriction) in fields {
                if let Some(restriction) = restriction
                    .as_str()
                    .filter(|value| !value.is_empty() && *value != "None")
                {
                    let access = match access.as_str() {
                        "read" | "Read" if combined => "ReadWrite",
                        "write" | "Write" if combined => continue,
                        "read" | "Read" => "Read",
                        "write" | "Write" => "Write",
                        name => name,
                    };

                    write!(markdown, " · {access}: {restriction}").expect("formatting a string");
                }
            }
        } else if let Some(restriction) = security
            .as_str()
            .filter(|value| !value.is_empty() && *value != "None")
        {
            write!(markdown, " · Restricted: {restriction}").expect("formatting a string");
        }
    }

    if object.get("deprecated").and_then(Value::as_bool) == Some(true) {
        markdown.push_str(" · Deprecated");
    }

    markdown
}

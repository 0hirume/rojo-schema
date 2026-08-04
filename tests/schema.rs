use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
    path::PathBuf,
    sync::OnceLock,
};

use jsonschema::{Draft, Validator};
use rojo_schema::{generate, Artifacts, Config};
use serde_json::{json, Value};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn rojo() -> PathBuf {
    config().rojo
}

fn source(name: &str) -> PathBuf {
    env::var_os(name)
        .unwrap_or_else(|| panic!("{name} is not set"))
        .into()
}

fn config() -> Config {
    Config {
        rojo: source("ROJO_SCHEMA_ROJO"),
        docs: source("ROJO_SCHEMA_DOCS"),
        project: root().join("dist/project.schema.json"),
        model: root().join("dist/model.schema.json"),
        manifest: root().join("dist/manifest.json"),
        coverage: root().join("dist/coverage.json"),
    }
}

fn artifacts() -> &'static Artifacts {
    static ARTIFACTS: OnceLock<Artifacts> = OnceLock::new();
    ARTIFACTS.get_or_init(|| generate(&config()).unwrap())
}

fn artifact(name: &str) -> Value {
    let artifacts = artifacts();
    let bytes = match name {
        "project.schema.json" => &artifacts.project,
        "model.schema.json" => &artifacts.model,
        "manifest.json" => &artifacts.manifest,
        "coverage.json" => &artifacts.coverage,
        _ => panic!("unknown artifact: {name}"),
    };
    serde_json::from_slice(bytes).unwrap()
}

fn fixture(name: &str) -> Value {
    serde_json::from_slice(&fs::read(root().join("tests/fixtures").join(name)).unwrap()).unwrap()
}

#[test]
fn schema_is_source_derived_and_class_aware() {
    let schema = artifact("project.schema.json");
    let model = artifact("model.schema.json");
    let manifest = artifact("manifest.json");
    let coverage = artifact("coverage.json");
    assert_eq!(
        schema["$schema"],
        "https://json-schema.org/draft/2020-12/schema"
    );
    assert_eq!(
        schema["$id"],
        format!(
            "{}/latest/project.schema.json",
            env!("CARGO_PKG_HOMEPAGE").trim_end_matches('/')
        )
    );
    assert_eq!(
        model["$id"],
        format!(
            "{}/latest/model.schema.json",
            env!("CARGO_PKG_HOMEPAGE").trim_end_matches('/')
        )
    );
    assert_eq!(manifest["projectSchemaId"], schema["$id"]);
    assert_eq!(manifest["modelSchemaId"], model["$id"]);
    assert_model_schema(&model);
    assert!(manifest["sources"]["rojo"]["version"].is_string());
    for source in manifest["sources"].as_object().unwrap().values() {
        assert!(source.get("path").is_none());
        assert!(source["repository"]
            .as_str()
            .is_some_and(|repository| repository.starts_with("https://")));
    }

    let definitions = schema["$defs"].as_object().unwrap();
    let character_auto_loads = &definitions["property/Players/CharacterAutoLoads"];
    assert_eq!(character_auto_loads["anyOf"][0]["type"], "boolean");
    assert_eq!(
        character_auto_loads["anyOf"][1]["$ref"],
        "#/$defs/value~1Bool"
    );
    assert_eq!(character_auto_loads["default"], true);
    let technology = &definitions["property/Lighting/Technology"];
    assert!(technology["default"].is_string());
    assert!(definitions["enum/Technology"]["enum"]
        .as_array()
        .unwrap()
        .contains(&technology["default"]));
    for prefix in [
        "rojo/",
        "serde/",
        "value/",
        "enum/",
        "property/",
        "properties/",
        "node/",
    ] {
        assert!(
            definitions.keys().any(|key| key.starts_with(prefix)),
            "missing $defs/{prefix}*"
        );
    }
    for variant in coverage["variantTypes"].as_array().unwrap() {
        let variant = variant.as_str().unwrap();
        assert!(
            definitions.contains_key(&format!("value/{variant}")),
            "missing traced VariantType {variant}"
        );
    }

    assert_eq!(
        definitions["node/Part"]["properties"]["$className"]["const"],
        "Part"
    );
    assert!(definitions["node/Part"].get("required").is_none());
    assert_eq!(
        definitions["node/DataModel"]["properties"]["Lighting"]["$ref"],
        "#/$defs/node~1Lighting"
    );
    assert!(definitions["node/Any"].get("anyOf").is_none());
    assert_eq!(
        definitions["node/Any"]["unevaluatedProperties"]["$ref"],
        "#/$defs/node~1Any"
    );
    assert_eq!(
        definitions["node/Any"]["allOf"].as_array().unwrap().len(),
        usize::try_from(coverage["counts"]["classes"].as_u64().unwrap()).unwrap() + 1
    );
    assert_eq!(
        definitions["properties/Part"]["properties"]["Name"]["$ref"],
        "#/$defs/property~1Instance~1Name"
    );
    assert_eq!(schema["properties"]["tree"]["$ref"], "#/$defs/node~1Any");

    let middleware = definitions["rojo/Middleware"]["anyOf"].as_array().unwrap();
    assert!(!middleware.is_empty());
    assert!(middleware.iter().all(|branch| branch["const"].is_string()));
    assert_eq!(coverage["counts"]["unclassified"], 0);
    assert_eq!(
        coverage["variantTypes"].as_array().unwrap().len(),
        usize::try_from(coverage["counts"]["variantTypes"].as_u64().unwrap()).unwrap()
    );
}

fn assert_model_schema(schema: &Value) {
    assert_eq!(schema["$ref"], "#/$defs/model~1Any");
    let part = &schema["$defs"]["model/Part"];
    assert_eq!(
        part["properties"]["properties"]["$ref"],
        "#/$defs/properties~1Part"
    );
    assert_eq!(
        part["properties"]["children"]["items"]["$ref"],
        "#/$defs/model~1Any"
    );
    assert_eq!(part["properties"]["ClassName"]["const"], "Part");
}

#[test]
fn every_coverage_item_is_classified_and_resolves() {
    let schema = artifact("project.schema.json");
    let coverage = artifact("coverage.json");
    let items = coverage["items"].as_array().unwrap();
    let allowed = BTreeSet::from([
        "matched",
        "api-only",
        "reflection-only",
        "type-conflict",
        "metadata-conflict",
        "non-projectable",
        "unsupported",
    ]);
    let mut classifications = BTreeMap::<&str, usize>::new();
    let mut projectable = BTreeMap::<&str, usize>::new();

    for item in items {
        let name = item["name"].as_str().unwrap();
        let kind = item["kind"].as_str().unwrap();
        let classification = item["classification"].as_str().unwrap();
        assert!(
            allowed.contains(classification),
            "unrecognized classification for {kind} {name}: {classification}"
        );
        assert!(
            !item["disposition"].as_str().unwrap().is_empty(),
            "missing disposition for {kind} {name}"
        );
        *classifications.entry(classification).or_default() += 1;

        if item["projectable"] == true {
            *projectable.entry(kind).or_default() += 1;
            let schema_ref = item["schemaRef"]
                .as_str()
                .unwrap_or_else(|| panic!("missing schemaRef for {kind} {name}"));
            let pointer = decode_fragment(schema_ref.strip_prefix('#').unwrap());
            let target = schema
                .pointer(&pointer)
                .unwrap_or_else(|| panic!("unresolved schemaRef for {kind} {name}: {schema_ref}"));

            if kind == "enum-item" {
                let (_, member) = name.rsplit_once('.').unwrap();
                assert!(
                    target["enum"].as_array().unwrap().contains(&json!(member)),
                    "enum member missing from schema: {name}"
                );
            }
        }
    }

    assert_eq!(
        usize::try_from(coverage["counts"]["apiItems"].as_u64().unwrap()).unwrap(),
        items.len()
    );
    for (classification, count) in classifications {
        assert_eq!(
            coverage["classifications"][classification],
            u64::try_from(count).unwrap()
        );
    }
    for (kind, count_key) in [
        ("class", "classes"),
        ("property", "properties"),
        ("enum", "enums"),
        ("enum-item", "enumItems"),
        ("variant-type", "variantTypes"),
    ] {
        assert_eq!(
            u64::try_from(projectable[kind]).unwrap(),
            coverage["counts"][count_key]
        );
    }

    assert_property_metadata(&schema);
}

fn decode_fragment(fragment: &str) -> String {
    let bytes = fragment.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            decoded.push((hex(bytes[index + 1]) << 4) | hex(bytes[index + 2]));
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(decoded).unwrap()
}

fn hex(byte: u8) -> u8 {
    match byte {
        b'0'..=b'9' => byte - b'0',
        b'A'..=b'F' => byte - b'A' + 10,
        b'a'..=b'f' => byte - b'a' + 10,
        _ => panic!("invalid percent escape"),
    }
}

fn assert_property_metadata(schema: &Value) {
    let property_definitions = schema["$defs"]
        .as_object()
        .unwrap()
        .iter()
        .filter(|(name, _)| name.starts_with("property/"))
        .map(|(_, schema)| schema)
        .collect::<Vec<_>>();
    assert!(property_definitions
        .iter()
        .any(|value| value["deprecated"] == true));
    assert!(property_definitions.iter().any(|value| {
        value["deprecationMessage"].is_string()
            && value["deprecationMessage"] == value["x-roblox-deprecation-message"]
    }));
    assert!(property_definitions
        .iter()
        .any(|value| value.get("x-rojo-alias-for").is_some()));
    assert!(property_definitions
        .iter()
        .any(|value| value.get("x-rojo-migration-targets").is_some()));
    assert!(property_definitions
        .iter()
        .any(|value| value["x-rojo-serialization"] == "does-not-serialize"));
    assert!(property_definitions
        .iter()
        .any(|value| value.get("x-roblox-security").is_some()));
}

#[test]
fn draft_schema_and_projects_validate() {
    let schema = artifact("project.schema.json");
    let validator = Validator::options()
        .with_draft(Draft::Draft202012)
        .build(&schema)
        .expect("generated schema must compile as Draft 2020-12");

    let valid_inline = json!({
        "name": "example",
        "tree": {
            "$className": "DataModel",
            "Workspace": {
                "Part": {
                    "$className": "Part",
                    "$properties": {
                        "Anchored": true,
                        "Position": [1, 2, 3]
                    }
                },
                "Terrain": {}
            }
        },
        "syncRules": [{ "pattern": "**/*.server.luau", "use": "serverScript" }]
    });
    assert_valid(&validator, &valid_inline, "inline positive fixture");

    let valid_local = fixture("valid.project.json");
    assert_valid(&validator, &valid_local, "local positive fixture");
    for relative in [
        "assets/project-templates/place/default.project.json",
        "test-projects/attributes/default.project.json",
        "test-projects/enums/default.project.json",
        "rojo-test/build-tests/infer_service_name/default.project.json",
        "rojo-test/build-tests/infer_starter_player/default.project.json",
        "rojo-test/build-tests/optional/default.project.json",
    ] {
        let value: Value =
            serde_json::from_slice(&fs::read(rojo().join(relative)).unwrap()).unwrap();
        assert_valid(&validator, &value, relative);
    }

    let invalid_local = fixture("invalid.project.json");
    assert!(!validator.is_valid(&invalid_local));
    assert!(!validator.is_valid(&json!({ "name": "missing tree" })));
    assert!(!validator.is_valid(&json!({
        "tree": { "$className": "Part", "$path": 42 }
    })));
}

#[test]
fn models_validate() {
    let schema = artifact("model.schema.json");
    let validator = Validator::options()
        .with_draft(Draft::Draft202012)
        .build(&schema)
        .expect("generated model schema must compile as Draft 2020-12");

    for relative in [
        "test-projects/json_model/src/implicit.model.json",
        "test-projects/json_model/src/children.model.json",
        "rojo-test/syncback-tests/schema_roundtrip/input-project/src/model.model.json",
        "rojo-test/serve-tests/ref_properties/ModelTarget.model.json",
    ] {
        let value: Value =
            serde_json::from_slice(&fs::read(rojo().join(relative)).unwrap()).unwrap();
        assert_valid(&validator, &value, relative);
    }

    let local = fixture("valid.model.json");
    assert_valid(&validator, &local, "local model fixture");
    assert!(!validator.is_valid(&json!({ "properties": {} })));
    assert!(!validator.is_valid(&json!({
        "className": "Part",
        "ClassName": "Part"
    })));
    assert!(!validator.is_valid(&json!({
        "className": "Part",
        "properties": { "NotAProperty": true }
    })));
}

#[test]
fn real_generation_is_byte_deterministic() {
    let first = artifacts();
    let second = generate(&config()).unwrap();
    assert_eq!(first.project, second.project);
    assert_eq!(first.model, second.model);
    assert_eq!(first.manifest, second.manifest);
    assert_eq!(first.coverage, second.coverage);
}

fn assert_valid(validator: &Validator, value: &Value, name: &str) {
    if !validator.is_valid(value) {
        let errors = validator
            .iter_errors(value)
            .map(|error| error.to_string())
            .collect::<Vec<_>>();
        panic!("{name} failed schema validation:\n{}", errors.join("\n"));
    }
}

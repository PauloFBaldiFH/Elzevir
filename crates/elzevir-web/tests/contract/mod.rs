// SPDX-License-Identifier: GPL-3.0-or-later
//! A minimal check of JSON answers against the API's `OpenAPI` document
//! (docs/specs/opac/api.md, tests), shared by the web tests and the end-to-end ones.
#![expect(
    clippy::indexing_slicing,
    reason = "a missing key of a JSON value reads as null and fails the check"
)]

/// The API's `OpenAPI` document.
pub(crate) fn document() -> serde_json::Value {
    serde_json::from_str(include_str!("../../src/api/openapi.json")).unwrap()
}

/// Checks `body`, the answer of `method` on `path` with `status`, against the document.
pub(crate) fn check(path: &str, method: &str, status: u16, body: &serde_json::Value) {
    let document = document();
    let schema = &document["paths"][path][method]["responses"][status.to_string()]["content"]["application/json"]
        ["schema"];
    assert!(
        !schema.is_null(),
        "{method} {path}: {status} is not in the document"
    );
    if let Err(error) = fits(&document, schema, body) {
        panic!("{method} {path} {status}: {error}");
    }
}

/// Whether `value` fits `schema` of the `OpenAPI` `document`: the part of the schema
/// language the document uses (`$ref`, `type`, `nullable`, `enum`, `required`,
/// `properties`, closed objects, `items`, `minimum`, `maximum`).
pub(crate) fn fits(
    document: &serde_json::Value,
    schema: &serde_json::Value,
    value: &serde_json::Value,
) -> Result<(), String> {
    use serde_json::Value;
    if let Some(reference) = schema["$ref"].as_str() {
        let name = reference.trim_start_matches("#/components/schemas/");
        return fits(document, &document["components"]["schemas"][name], value);
    }
    if value.is_null() {
        return if schema["nullable"] == true {
            Ok(())
        } else {
            Err(format!("null where {schema} is not nullable"))
        };
    }
    if let Some(allowed) = schema["enum"].as_array()
        && !allowed.contains(value)
    {
        return Err(format!("{value} is not one of {allowed:?}"));
    }
    let typed = match schema["type"].as_str() {
        Some("string") => value.is_string(),
        Some("integer") => value.is_i64() || value.is_u64(),
        Some("boolean") => value.is_boolean(),
        Some("array") => value.is_array(),
        Some("object") => value.is_object(),
        _ => true,
    };
    if !typed {
        return Err(format!("{value} is not a {}", schema["type"]));
    }
    if let Some(number) = value.as_i64()
        && (schema["minimum"]
            .as_i64()
            .is_some_and(|minimum| number < minimum)
            || schema["maximum"]
                .as_i64()
                .is_some_and(|maximum| number > maximum))
    {
        return Err(format!("{number} is out of range in {schema}"));
    }
    if let Value::Array(values) = value {
        for value in values {
            fits(document, &schema["items"], value)?;
        }
    }
    if let (Value::Object(fields), Some(properties)) = (value, schema["properties"].as_object()) {
        for required in schema["required"].as_array().into_iter().flatten() {
            let name = required.as_str().unwrap();
            if !fields.contains_key(name) {
                return Err(format!("{name} missing from {value}"));
            }
        }
        for (name, field) in fields {
            let Some(property) = properties.get(name) else {
                return Err(format!("{name} is not in {schema}"));
            };
            fits(document, property, field).map_err(|error| format!("{name}: {error}"))?;
        }
    }
    Ok(())
}

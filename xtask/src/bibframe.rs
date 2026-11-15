// SPDX-License-Identifier: GPL-3.0-or-later
//! Generates the BIBFRAME classes and properties in `elzevir-data` from
//! `data/loc/bibframe.json`, which `tools/bibframe-extractor/extract.pl` writes from the
//! pinned Library of Congress ontology (docs/specs/catalogo/bibframe-completo.md, item 2).

use std::fmt::Write as _;

use serde_json::Value;

/// `bibframe.rs`.
pub(crate) fn generate(json: &Value, from: &str, header: &str) -> Result<String, String> {
    let source = |key: &str| text(json, &format!("/source/{key}"));
    let mut out = header.to_owned();
    writeln!(
        out,
        "//! The Library of Congress BIBFRAME ontology, generated from {from}.\n\n\
         use crate::bibframe::{{Class, Ontology, Property, PropertyKind}};\n\n\
         /// The pinned ontology.\n\
         pub(crate) const ONTOLOGY: Ontology = Ontology {{ version: {:?}, version_iri: {:?}, issued: {:?}, source_url: {:?}, source_sha256: {:?} }};",
        source("version")?,
        source("version_iri")?,
        source("issued")?,
        source("url")?,
        source("sha256")?,
    )
    .map_err(fmt_error)?;

    out.push_str(
        "\n/// Every class, sorted by name.\n#[rustfmt::skip]\npub(crate) static CLASSES: &[Class] = &[\n",
    );
    for class in list(json, "/classes")? {
        writeln!(
            out,
            "    Class {{ name: {:?}, label: {:?}, definition: {:?}, super_classes: &{:?} }},",
            text(class, "/name")?,
            text(class, "/label")?,
            text(class, "/definition")?,
            names(class, "/sub_class_of")?,
        )
        .map_err(fmt_error)?;
    }
    out.push_str(
        "];\n\n/// Every property, sorted by name.\n#[rustfmt::skip]\npub(crate) static PROPERTIES: &[Property] = &[\n",
    );
    for property in list(json, "/properties")? {
        let name = text(property, "/name")?;
        let kind = match text(property, "/kind")? {
            "object" => "Object",
            "datatype" => "Datatype",
            "symmetric" => "Symmetric",
            other => return Err(format!("property {name}: unknown kind `{other}`")),
        };
        let inverse = match names(property, "/inverse_of")?.as_slice() {
            [] => "None".to_owned(),
            [one] => format!("Some({one:?})"),
            _ => return Err(format!("property {name}: more than one inverse")),
        };
        writeln!(
            out,
            "    Property {{ name: {name:?}, label: {:?}, definition: {:?}, kind: PropertyKind::{kind}, domains: &{:?}, ranges: &{:?}, super_properties: &{:?}, inverse_of: {inverse}, used_with: &{:?}, suggested_values: &{:?} }},",
            text(property, "/label")?,
            text(property, "/definition")?,
            names(property, "/domain")?,
            names(property, "/range")?,
            names(property, "/sub_property_of")?,
            names(property, "/used_with")?,
            names(property, "/suggested_value")?,
        )
        .map_err(fmt_error)?;
    }
    out.push_str("];\n");
    Ok(out)
}

fn text<'a>(json: &'a Value, pointer: &str) -> Result<&'a str, String> {
    json.pointer(pointer)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("bibframe.json: `{pointer}` is not text"))
}

fn list<'a>(json: &'a Value, pointer: &str) -> Result<&'a [Value], String> {
    json.pointer(pointer)
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .ok_or_else(|| format!("bibframe.json: `{pointer}` is not a list"))
}

fn names<'a>(json: &'a Value, pointer: &str) -> Result<Vec<&'a str>, String> {
    list(json, pointer)?
        .iter()
        .map(|name| {
            name.as_str()
                .ok_or_else(|| format!("bibframe.json: `{pointer}` holds a non-text value"))
        })
        .collect()
}

fn fmt_error(e: std::fmt::Error) -> String {
    format!("formatting generated code failed: {e}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ontology(property: &str) -> Value {
        serde_json::from_str(&format!(
            r#"{{"source": {{"version": "3.0.1", "version_iri": "v", "issued": "d", "url": "u", "sha256": "s"}},
                "classes": [{{"name": "Text", "label": "Text", "definition": "A \"text\"", "sub_class_of": ["Work"]}}],
                "properties": [{property}]}}"#
        ))
        .unwrap()
    }

    const PROPERTY: &str = r#"{"name": "hasItem", "label": "Has item", "definition": "d", "kind": "object",
        "domain": ["Instance"], "range": ["Item"], "sub_property_of": [], "inverse_of": ["itemOf"],
        "used_with": [], "suggested_value": []}"#;

    #[test]
    fn classes_and_properties_become_rows() {
        let source = generate(&ontology(PROPERTY), "data/loc/bibframe.json", "").unwrap();
        assert!(source.contains(
            r#"Class { name: "Text", label: "Text", definition: "A \"text\"", super_classes: &["Work"] }"#
        ));
        assert!(source.contains(r#"kind: PropertyKind::Object, domains: &["Instance"], ranges: &["Item"], super_properties: &[], inverse_of: Some("itemOf")"#));
    }

    #[test]
    fn an_unknown_kind_is_an_error() {
        let property = PROPERTY.replace("\"object\"", "\"functional\"");
        let error = generate(&ontology(&property), "f", "").unwrap_err();
        assert!(error.contains("unknown kind"), "{error}");
    }
}

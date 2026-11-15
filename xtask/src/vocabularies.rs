// SPDX-License-Identifier: GPL-3.0-or-later
//! Generates the Library of Congress vocabularies in `elzevir-data` from
//! `data/loc/vocabularies.json`, which `tools/bibframe-extractor/vocabularies.pl` writes
//! from the pinned bulk downloads (docs/specs/catalogo/bibframe-completo.md, item 11).
//! One file per downloaded file, so no generated file grows past the size limit.

use std::fmt::Write as _;

use serde_json::Value;

/// A generated file: its name in the output directory and its source.
pub(crate) struct File {
    /// Name relative to the output directory.
    pub(crate) name: String,
    /// Unformatted Rust.
    pub(crate) source: String,
}

/// `vocabularies.rs`, the index, and one `vocabulary_<file>.rs` per downloaded file.
pub(crate) fn files(
    json: &Value,
    from: &str,
    header: &dyn Fn(&str) -> String,
) -> Result<Vec<File>, String> {
    let sources = list(json, "/sources")?;
    let schemes = list(json, "/schemes")?;
    let mut files = Vec::new();
    let mut index = header(from);
    writeln!(
        index,
        "//! The Library of Congress vocabularies, generated from {from}.\n\n\
         use crate::vocabulary::{{Scheme, VocabularySource}};\n\n\
         /// The downloaded files.\n\
         pub(crate) static SOURCES: &[VocabularySource] = &["
    )
    .map_err(fmt_error)?;
    let mut all = Vec::new();
    for source in sources {
        let file = text(source, "/file")?;
        let stem = file
            .rsplit('/')
            .next()
            .and_then(|name| name.split('.').next())
            .filter(|stem| stem.chars().all(|c| c.is_ascii_alphanumeric()))
            .ok_or_else(|| format!("vocabularies.json: odd source file {file}"))?;
        writeln!(
            index,
            "    VocabularySource {{ file: {file:?}, url: {:?}, sha256: {:?} }},",
            text(source, "/url")?,
            text(source, "/sha256")?,
        )
        .map_err(fmt_error)?;
        let module = format!("vocabulary_{}", stem.to_ascii_lowercase());
        // The bulk "cataloging" file holds many schemes; the others hold the scheme they
        // are named after.
        let mine: Vec<&Value> = schemes
            .iter()
            .filter(|scheme| {
                let name = text(scheme, "/name").unwrap_or_default();
                if stem == "cataloging" {
                    !sources.iter().any(|other| {
                        text(other, "/file").is_ok_and(|f| f.contains(&format!("/{name}.")))
                    })
                } else {
                    name == stem
                }
            })
            .collect();
        let mut out = header(from);
        writeln!(
            out,
            "//! Schemes from `{file}`.\n\n\
             use crate::vocabulary::{{Concept, Scheme}};\n"
        )
        .map_err(fmt_error)?;
        for scheme in &mine {
            let name = text(scheme, "/name")?;
            let constant = name.to_ascii_uppercase().replace('-', "_");
            writeln!(
                out,
                "/// `{name}`.\n#[rustfmt::skip]\npub(crate) static {constant}: Scheme = Scheme {{ name: {name:?}, iri: {:?}, concepts: &[",
                text(scheme, "/iri")?,
            )
            .map_err(fmt_error)?;
            for concept in list(scheme, "/concepts")? {
                writeln!(
                    out,
                    "    Concept {{ code: {:?}, label: {:?}, deprecated: {} }},",
                    text(concept, "/code")?,
                    text(concept, "/label")?,
                    concept
                        .pointer("/deprecated")
                        .and_then(Value::as_bool)
                        .ok_or("vocabularies.json: `deprecated` is not a boolean")?,
                )
                .map_err(fmt_error)?;
            }
            out.push_str("] };\n\n");
            all.push((name, format!("&super::{module}::{constant}")));
        }
        files.push(File {
            name: format!("{module}.rs"),
            source: out,
        });
    }
    if all.len() != schemes.len() {
        return Err(format!(
            "vocabularies.json: {} schemes, {} placed in a file",
            schemes.len(),
            all.len()
        ));
    }
    all.sort_unstable();
    let all: Vec<String> = all.into_iter().map(|(_, path)| path).collect();
    writeln!(
        index,
        "];\n\n/// Every scheme, sorted by name.\npub(crate) static SCHEMES: &[&Scheme] = &[{}];",
        all.join(", ")
    )
    .map_err(fmt_error)?;
    files.push(File {
        name: "vocabularies.rs".to_owned(),
        source: index,
    });
    Ok(files)
}

fn text<'a>(json: &'a Value, pointer: &str) -> Result<&'a str, String> {
    json.pointer(pointer)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("vocabularies.json: `{pointer}` is not text"))
}

fn list<'a>(json: &'a Value, pointer: &str) -> Result<&'a [Value], String> {
    json.pointer(pointer)
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .ok_or_else(|| format!("vocabularies.json: `{pointer}` is not a list"))
}

fn fmt_error(e: std::fmt::Error) -> String {
    format!("formatting generated code failed: {e}")
}

#[cfg(test)]
mod tests {
    use super::*;

    const JSON: &str = r#"{
        "sources": [
            {"file": "reference/loc-vocabularies/cataloging.skosrdf.nt.gz", "url": "u", "sha256": "s"},
            {"file": "reference/loc-vocabularies/languages.skosrdf.nt.gz", "url": "u", "sha256": "s"}
        ],
        "schemes": [
            {"name": "contentTypes", "iri": "http://id.loc.gov/vocabulary/contentTypes",
             "concepts": [{"code": "txt", "label": "text", "deprecated": false}]},
            {"name": "languages", "iri": "http://id.loc.gov/vocabulary/languages",
             "concepts": [{"code": "por", "label": "Portuguese", "deprecated": false}]}
        ]}"#;

    #[test]
    fn each_scheme_goes_to_the_file_it_came_from() {
        let json = serde_json::from_str(JSON).unwrap();
        let files = files(&json, "f", &|_| String::new()).unwrap();
        let names: Vec<_> = files.iter().map(|file| file.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "vocabulary_cataloging.rs",
                "vocabulary_languages.rs",
                "vocabularies.rs"
            ]
        );
        assert!(files[0].source.contains("static CONTENTTYPES: Scheme"));
        assert!(!files[0].source.contains("LANGUAGES"));
        assert!(
            files[1]
                .source
                .contains(r#"Concept { code: "por", label: "Portuguese", deprecated: false }"#)
        );
        assert!(
            files[2]
                .source
                .contains("&super::vocabulary_languages::LANGUAGES")
        );
    }
}

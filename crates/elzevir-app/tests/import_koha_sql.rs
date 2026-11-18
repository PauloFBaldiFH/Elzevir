// SPDX-License-Identifier: GPL-3.0-or-later
//! Grammar, inventory closure and value-free diagnostics at the Koha SQL edge.

use std::io::{BufReader, Read};

use elzevir_app::import::koha_sql::{
    DumpProblem, MAX_SQL_STATEMENT_BYTES, MAX_SQL_VALUE_BYTES, prepare,
};
use elzevir_marc::Field;

const XML: &str = r#"<record xmlns="http://www.loc.gov/MARC21/slim">
<leader>00000nam a2200000 i 4500</leader>
<datafield tag="245" ind1="1" ind2="0"><subfield code="a">Obra fictícia</subfield></datafield>
<datafield tag="999" ind1=" " ind2=" "><subfield code="c">42</subfield><subfield code="d">88</subfield></datafield>
</record>"#;

fn metadata(xml: &str) -> String {
    format!("INSERT INTO biblio_metadata (biblionumber, metadata) VALUES (42,'{xml}');")
}

#[test]
fn declares_reordered_columns_and_joins_items_before_multiline_metadata() {
    let source = format!(
        r#"﻿-- retained only as stream
/*!40101 SET character_set_client = utf8mb4 */;
SET NAMES utf8mb4;
DROP TABLE IF EXISTS `koha_fixture`.`items`;
CREATE TABLE IF NOT EXISTS `koha_fixture`.`items` (
    `barcode` varchar(20), `itemcallnumber` varchar(255),
    biblionumber int, itemnumber int, price decimal(8,2),
    UNIQUE KEY `test_idx` (`barcode`), PRIMARY KEY (itemnumber)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
LOCK TABLES `koha_fixture`.`items` WRITE;
INSERT INTO `koha_fixture`.`items` VALUES
 ('FICTION-1','Estante ''X''\\Y\nAndar\t2',42,11,2.14),
 ("FICTION-2","Shelf \"B\"",42,12,NULL);
UNLOCK TABLES;
CREATE TABLE biblio_metadata (format varchar(16), metadata longtext, biblionumber int);
INSERT INTO biblio_metadata VALUES ('marcxml','{XML}',42);
"#
    );
    let bytes = BufReader::with_capacity(1, source.as_bytes());
    let records = prepare(bytes).unwrap();
    assert_eq!(records.len(), 1);
    let (record, offset) = records.first().unwrap();
    assert_eq!(*offset, source.find("('marcxml'").unwrap() as u64);
    let fields: Vec<_> = record
        .fields
        .iter()
        .map(|field| *field.tag().as_bytes())
        .collect();
    assert_eq!(fields, [*b"245", *b"952", *b"952", *b"999"]);
    let items: Vec<_> = record
        .fields
        .iter()
        .filter_map(|field| {
            if let Field::Data { tag, subfields, .. } = field
                && tag.as_bytes() == b"952"
            {
                Some(subfields)
            } else {
                None
            }
        })
        .collect();
    let value = |item: usize, code| {
        items[item]
            .iter()
            .find(|field| field.code == code)
            .map(|field| field.value.as_slice())
    };
    assert_eq!(value(0, b'o'), Some("Estante 'X'\\Y\nAndar\t2".as_bytes()));
    assert_eq!(value(0, b'g'), Some(b"2.14".as_slice()));
    assert_eq!(value(0, b'9'), Some(b"11".as_slice()));
    assert_eq!(value(1, b'o'), Some(b"Shelf \"B\"".as_slice()));
    assert_eq!(value(1, b'g'), None);
    assert_eq!(value(1, b'9'), Some(b"12".as_slice()));
}

#[test]
fn structural_and_relational_errors_report_offsets_instead_of_source_values() {
    let metadata = metadata(XML);
    let items = "INSERT INTO items (itemnumber,biblionumber,barcode) VALUES ";
    for (source, problem, place) in [
        (
            format!("{metadata}\n{items}(7,99,'SENTINEL');"),
            DumpProblem::Orphan,
            "(7,99",
        ),
        (
            format!("{metadata}\n{items}(7,42,'X'),(7,42,'SENTINEL');"),
            DumpProblem::Duplicate("itemnumber or barcode"),
            "(7,42,'SENTINEL'",
        ),
        (
            format!("{metadata}\n{items}(7,42,'SENTINEL'),(8,42,' SENTINEL ');"),
            DumpProblem::Duplicate("itemnumber or barcode"),
            "(8,42",
        ),
        (
            format!("{metadata}\n{metadata}"),
            DumpProblem::Duplicate("bibliographic key"),
            "(42,",
        ),
    ] {
        let error = prepare(source.as_bytes()).unwrap_err();
        assert_eq!(error.problem, problem);
        assert_eq!(error.offset, source.rfind(place).unwrap() as u64);
        assert!(!error.to_string().contains("SENTINEL"));
    }
    let missing =
        prepare("INSERT INTO biblio_metadata VALUES (42,'SENTINEL');".as_bytes()).unwrap_err();
    assert_eq!(missing.problem, DumpProblem::MissingColumns);
    let unsupported = format!("{metadata} DELETE FROM items;");
    assert_eq!(
        prepare(unsupported.as_bytes()).unwrap_err().problem,
        DumpProblem::Unsupported
    );
}

#[test]
fn rejects_conflicting_embedded_items_and_preserves_equivalent_952_fields() {
    let xml = XML.replace(
        "</record>",
        r#"
<datafield tag="952" ind1="1" ind2="0">
<subfield code="p">FICTION-7</subfield><subfield code="9">7</subfield>
<subfield code="6">original sort value</subfield><subfield code="g">2.10</subfield>
</datafield>
</record>"#,
    );
    let meta = metadata(&xml);
    let source = |barcode, price, id| {
        format!(
            "{meta} INSERT INTO items (itemnumber,biblionumber,barcode,price,notforloan) VALUES ({id},42,'{barcode}',{price},0);"
        )
    };
    let expected = prepare(meta.as_bytes()).unwrap();
    let equivalent = prepare(source("FICTION-7", "2.1", 7).as_bytes()).unwrap();
    assert_eq!(equivalent, expected);
    for input in [
        source("FICTION-7", "2.1", 8),
        source("FICTION-OTHER", "2.1", 7),
        source("FICTION-7", "3.0", 7),
    ] {
        assert!(matches!(
            prepare(input.as_bytes()).unwrap_err().problem,
            DumpProblem::Conflict(_)
        ));
    }
}

#[test]
fn ignores_private_literals_without_applying_the_retained_value_limit() {
    let marker = "SYNTHETIC_PRIVATE_SENTINEL";
    let payload = "x".repeat(MAX_SQL_VALUE_BYTES);
    let source = format!(
        "INSERT INTO borrowers (name) VALUES ('{marker}; CREATE TABLE issues (n int); {payload}');\n{}",
        metadata(XML),
    );
    let records = prepare(source.as_bytes()).unwrap();
    assert_eq!(records.len(), 1);
    assert!(!format!("{records:?}").contains(marker));
    let bad = format!("INSERT INTO borrowers VALUES ('{marker}\\k');");
    let error = prepare(bad.as_bytes()).unwrap_err();
    assert!(!format!("{error:?} {error}").contains(marker));
    let large_metadata = metadata(&payload);
    assert_eq!(
        prepare(large_metadata.as_bytes()).unwrap_err().problem,
        DumpProblem::Metadata
    );
    let large_metadata = metadata(&format!("{payload}x"));
    assert_eq!(
        prepare(large_metadata.as_bytes()).unwrap_err().problem,
        DumpProblem::Limit("retained value bytes")
    );
}

#[test]
fn rejects_oversize_ignored_statements_while_reading_them() {
    let prefix = b"INSERT INTO accountlines (note) VALUES ('";
    let buffer = BufReader::new(prefix.as_slice().chain(std::io::repeat(b'x')));
    let error = prepare(buffer).unwrap_err();
    assert_eq!(error.offset, MAX_SQL_STATEMENT_BYTES);
    assert_eq!(error.problem, DumpProblem::Limit("statement bytes"));
}

#[test]
fn empty_and_comment_only_input_make_an_empty_preparation() {
    assert!(
        prepare(b" ; # ignored\r\n  /* export */ ; ".as_slice())
            .unwrap()
            .is_empty()
    );
    assert!(prepare(b"".as_slice()).unwrap().is_empty());
}

#[test]
fn enforces_the_iso_field_limit_on_the_metadata_before_submission() {
    let fields = "<controlfield tag=\"001\">1</controlfield>"
        .repeat(elzevir_marc::Limits::default().max_fields);
    let xml = XML.replace("</record>", &format!("{fields}</record>"));
    let error = prepare(metadata(&xml).as_bytes()).unwrap_err();
    assert_eq!(error.problem, DumpProblem::Limit("MARC field count"));
}

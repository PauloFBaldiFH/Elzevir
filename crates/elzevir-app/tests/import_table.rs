// SPDX-License-Identifier: GPL-3.0-or-later
//! Logical rows, source positions and bounded decoding for spreadsheets.
use std::io::BufReader;

use elzevir_app::import::table::{
    Delimiter, Encoding, ErrorKind, MAX_TABLE_CELL_BYTES, MAX_TABLE_COLUMNS, MAX_TABLE_ROW_BYTES,
    TableReader, write_model_header,
};

#[test]
fn logical_rows_keep_repeated_headers_empty_cells_and_multiline_values() {
    let input = b"\xef\xbb\xbftitle,title,\r\n\r\n\"One, \"\"two\"\"\r\nthree\",,\n";
    let tiny = BufReader::with_capacity(1, input.as_slice());
    let rows: Vec<_> = TableReader::new(tiny, Delimiter::Comma, Encoding::Utf8)
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    let first = rows.first().unwrap();
    let second = rows.get(1).unwrap();
    assert_eq!((first.number, first.offset), (1, 3));
    assert_eq!(first.cells, ["title", "title", ""]);
    assert_eq!((second.number, second.offset), (2, 19));
    assert_eq!(second.cells, ["One, \"two\"\r\nthree", "", ""]);
    assert_eq!(rows.len(), 2);
}

#[test]
fn tsv_decodes_latin1_only_when_selected() {
    let input = b"title\tclassification\nHist\xf3ria\t7";
    let reader = |encoding| TableReader::new(input.as_slice(), Delimiter::Tab, encoding).unwrap();
    let last = reader(Encoding::Latin1).last().unwrap().unwrap();
    assert_eq!(last.cells, ["História", "7"]);
    let error = reader(Encoding::Utf8).last().unwrap().unwrap_err();
    assert_eq!((error.row, error.column, error.offset), (2, 1, 21));
    assert!(matches!(error.kind, ErrorKind::InvalidUtf8));
}

#[test]
fn reports_malformed_quoting_at_the_logical_record_and_stops() {
    let input = b"name,other\n\"first\r\nline\",\"unclosed";
    let mut reader = TableReader::new(input.as_slice(), Delimiter::Comma, Encoding::Utf8).unwrap();
    assert_eq!(reader.next().unwrap().unwrap().cells, ["name", "other"]);
    let error = reader.next().unwrap().unwrap_err();
    assert_eq!((error.row, error.column, error.offset), (2, 2, 11));
    assert!(matches!(error.kind, ErrorKind::UnclosedQuote));
    assert!(reader.next().is_none());
    for invalid in [b"some\"quote".as_slice(), b"\"closed\"suffix".as_slice()] {
        let error = TableReader::new(invalid, Delimiter::Comma, Encoding::Utf8)
            .unwrap()
            .next()
            .unwrap()
            .unwrap_err();
        assert!(matches!(error.kind, ErrorKind::InvalidQuote));
    }
}

#[test]
fn stops_at_the_first_excess_cell_or_decoded_byte() {
    for (input, encoding, column, limit) in [
        (
            vec![b','; MAX_TABLE_COLUMNS],
            Encoding::Utf8,
            MAX_TABLE_COLUMNS + 1,
            "columns",
        ),
        (
            vec![0xe9; MAX_TABLE_CELL_BYTES / 2 + 1],
            Encoding::Latin1,
            1,
            "cell bytes",
        ),
        (
            std::iter::repeat_n(
                "x".repeat(MAX_TABLE_CELL_BYTES),
                MAX_TABLE_ROW_BYTES / MAX_TABLE_CELL_BYTES,
            )
            .collect::<Vec<_>>()
            .join(",")
            .into_bytes()
            .into_iter()
            .chain(b",x".iter().copied())
            .collect(),
            Encoding::Utf8,
            MAX_TABLE_ROW_BYTES / MAX_TABLE_CELL_BYTES + 1,
            "row bytes",
        ),
    ] {
        let mut rows = TableReader::new(input.as_slice(), Delimiter::Comma, encoding).unwrap();
        let error = rows.next().unwrap().unwrap_err();
        assert_eq!((error.row, error.column, error.offset), (1, column, 0));
        assert!(matches!(error.kind, ErrorKind::Limit(name) if name == limit));
        assert!(rows.next().is_none());
    }
}

#[test]
fn model_is_versioned_and_qualifies_every_ontology_column() {
    let mut header = Vec::new();
    write_model_header(&mut header, Delimiter::Tab).unwrap();
    let row = TableReader::new(header.as_slice(), Delimiter::Tab, Encoding::Utf8)
        .unwrap()
        .next()
        .unwrap()
        .unwrap();
    let ontology = elzevir_data::bibframe::ontology();
    assert_eq!(row.cells.first().unwrap(), "elzevir-model:1");
    assert_eq!(
        row.cells.get(1).unwrap(),
        &format!("bibframe:{}:{}", ontology.version, ontology.source_sha256)
    );
    for entity in elzevir_data::bibframe::Entity::ALL {
        assert!(
            row.cells
                .contains(&format!("{}.@type", entity.class_name()))
        );
        for property in elzevir_data::bibframe::properties_of(entity) {
            assert!(
                row.cells
                    .contains(&format!("{}.{}", entity.class_name(), property.name))
            );
        }
    }
    assert!(row.cells.contains(&"Item.catalog.barcode".to_owned()));
}

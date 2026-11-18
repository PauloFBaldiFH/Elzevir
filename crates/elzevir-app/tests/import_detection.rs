// SPDX-License-Identifier: GPL-3.0-or-later
//! Synthetic prefix evidence for the universal import detector; no source is executed.

use elzevir_app::import_detection::{Evidence, ImportFormat, MAX_DETECTION_BYTES, detect_format};

#[test]
fn every_format_has_content_evidence() {
    let examples: &[(&[u8], ImportFormat, Evidence)] = &[
        (b"00080nam a2200037 a 4500", ImportFormat::Iso2709, Evidence::IsoLeader),
        (b"<record xmlns='http://www.loc.gov/MARC21/slim'>", ImportFormat::MarcXml, Evidence::MarcXmlNamespace),
        (b"=LDR  00000nam a2200000 a 4500\n", ImportFormat::MarcText, Evidence::MarcTextLeader),
        (b"title,barcode\nSynthetic,SYN-1\n", ImportFormat::Delimited, Evidence::DelimitedRows),
        (b"CREATE TABLE `biblio_metadata` (metadata text);", ImportFormat::KohaSqlDump, Evidence::KohaCatalogTables),
        (b"INSERT INTO items (itemnumber) VALUES (1);", ImportFormat::KohaSqlDump, Evidence::KohaCatalogTables),
        (b"CREATE TABLE biblio_metadata (metadata text); CREATE TABLE items (id int); CREATE TABLE borrowers (id int); CREATE TABLE issues (id int); CREATE TABLE systempreferences (name text);", ImportFormat::KohaFullBackup, Evidence::KohaSchemaTables),
        (b"unrecognized synthetic content", ImportFormat::Unknown, Evidence::Inconclusive),
    ];
    for &(bytes, format, evidence) in examples {
        let result = detect_format(bytes);
        assert_eq!(result.format, format, "{result:?}");
        assert_eq!(result.evidence, evidence, "{result:?}");
    }
}

#[test]
fn iso_recovery_and_empty_input_still_belong_to_the_existing_reader() {
    for bytes in [b"".as_slice(), b" \r\n\t"] {
        let detected = detect_format(bytes);
        assert_eq!(detected.format, ImportFormat::Iso2709);
        assert_eq!(detected.evidence, Evidence::EmptyInput);
    }
    assert_eq!(
        detect_format(b"00080nam a2200037 a 4500").format,
        ImportFormat::Iso2709
    );
    let recovery = detect_format(b"damaged\x1d00080nam a2200037 a 4500");
    assert_eq!(recovery.format, ImportFormat::Iso2709);
    assert_eq!(recovery.evidence, Evidence::IsoLeader);
}

#[test]
fn xml_namespace_and_root_are_required_instead_of_a_tag_somewhere() {
    for bytes in [
        b"\xef\xbb\xbf <?xml version='1.0'?><!-- synthetic --><m:collection xmlns:m='http://www.loc.gov/MARC21/slim'>".as_slice(),
        b"<record unrelated='>' xmlns=\"http://www.loc.gov/MARC21/slim\">",
    ] {
        assert_eq!(detect_format(bytes).format, ImportFormat::MarcXml);
    }
    for bytes in [
        b"<not-marc><record xmlns='http://www.loc.gov/MARC21/slim'>".as_slice(),
        b"<m:record xmlns:q='http://www.loc.gov/MARC21/slim'>",
        b"<record xmlns='http://example.invalid/not-marc'>",
        b"<record xmlns='http://www.loc.gov/MARC21/slim' xmlns='different'>",
        b"<?xml version='1.0'?><record xmlns=",
    ] {
        assert_eq!(detect_format(bytes).format, ImportFormat::Unknown);
    }
}

#[test]
fn sql_container_wins_over_xml_values_and_respects_declared_names() {
    let sql = b"-- synthetic header\nSET NAMES utf8mb4;\nCREATE TABLE `biblio_metadata` (metadata text);\nINSERT INTO biblio_metadata VALUES ('<record xmlns=\"http://www.loc.gov/MARC21/slim\">');";
    assert_eq!(detect_format(sql).format, ImportFormat::KohaSqlDump);
    let qualified = b"/* synthetic */ cReAtE tAbLe IF NOT EXISTS `synthetic`.`items` (id int);";
    assert_eq!(detect_format(qualified).format, ImportFormat::KohaSqlDump);
    for sql in [
        b"CREATE TABLE unrelated (value text); INSERT INTO unrelated VALUES ('CREATE TABLE biblio_metadata (id int);');".as_slice(),
        b"-- CREATE TABLE biblio_metadata (id int);",
        b"CREATE TABLE biblio_metadata_extra (id int);",
        b"CREATE TABLE borrowers (id int);",
    ] {
        assert_eq!(detect_format(sql).format, ImportFormat::Unknown);
    }
    let partial = b"CREATE TABLE borrowers (id int); CREATE TABLE items (id int);";
    assert_eq!(detect_format(partial).format, ImportFormat::KohaSqlDump);
}

#[test]
fn delimited_evidence_respects_quotes_bom_and_separator_ambiguity() {
    for bytes in [
        b"\xef\xbb\xbftitle\tbarcode\r\nSynthetic\tSYN-1\r\n".as_slice(),
        b"title;barcode\nSynthetic;SYN-1",
        b"\"title,with;punctuation\",barcode\n\"Synthetic\nmultiline\",SYN-1\n",
        b"title,barcode\n\"Synthetic \"\"quoted\"\" title\",SYN-1\n",
        b"title,barcode\nSynthetic caf\xe9,SYN-1\n",
        b"12345678901234567890,1\n12345678901234567891,2\n",
    ] {
        assert_eq!(detect_format(bytes).format, ImportFormat::Delimited);
    }
    let ambiguous = detect_format(b"title,part;barcode\nSynthetic,one;SYN-1\n");
    assert_eq!(ambiguous.format, ImportFormat::Unknown);
    assert_eq!(ambiguous.evidence, Evidence::AmbiguousDelimiter);
    for bytes in [
        b"just one line,with a comma".as_slice(),
        b"a,b\n\"unfinished",
        b"12345678901234567",
    ] {
        assert_eq!(detect_format(bytes).format, ImportFormat::Unknown);
    }
}

#[test]
fn containers_are_recognized_without_reading_or_extracting_members() {
    for bytes in [b"PK\x03\x04".as_slice(), b"PK\x05\x06", b"PK\x07\x08"] {
        let result = detect_format(bytes);
        assert_eq!(result.format, ImportFormat::Unknown);
        assert_eq!(result.evidence, Evidence::ZipSignature);
    }
    let gzip = detect_format(b"\x1f\x8b\x08");
    assert_eq!(gzip.format, ImportFormat::Unknown);
    assert_eq!(gzip.evidence, Evidence::GzipSignature);
}

#[test]
fn prefixes_are_bounded_and_truncation_never_panics() {
    let mut prefix = b"-- ".to_vec();
    prefix.extend(std::iter::repeat_n(b'x', MAX_DETECTION_BYTES));
    let before = detect_format(&prefix);
    prefix.extend(b"\nCREATE TABLE biblio_metadata (metadata text);");
    assert_eq!(detect_format(&prefix), before);
    for example in [
        b"CREATE TABLE `biblio_metadata` (metadata text);".as_slice(),
        b"<?xml version='1.0'?><!-- comment --><m:record xmlns:m='http://www.loc.gov/MARC21/slim'>",
        b"a,b\n\"x\"\"y\",z\n",
    ] {
        for end in 0..=example.len() {
            if let Some(prefix) = example.get(..end) {
                std::hint::black_box(detect_format(prefix));
            }
        }
    }
    for value in 0..=u8::MAX {
        std::hint::black_box(detect_format(&[value; 32]));
    }
}

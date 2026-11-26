// SPDX-License-Identifier: GPL-3.0-or-later
//! The four formats on records built field by field.

use elzevir_domain::catalog::InstanceId;
use elzevir_marc::{Field, Record, Subfield, Tag};

use super::{CitationFormat, cite};

fn data(tag: [u8; 3], indicators: [u8; 2], subfields: &[(u8, &str)]) -> Field {
    Field::Data {
        tag: Tag::new(tag),
        indicators,
        subfields: subfields
            .iter()
            .map(|(code, value)| Subfield {
                code: *code,
                value: value.as_bytes().to_vec(),
            })
            .collect(),
    }
}

fn control(tag: [u8; 3], value: &str) -> Field {
    Field::Control {
        tag: Tag::new(tag),
        value: value.as_bytes().to_vec(),
    }
}

fn leader(type_and_level: [u8; 2]) -> [u8; 24] {
    let mut leader = *b"00000nam a2200000 a 4500";
    leader[6] = type_and_level[0];
    leader[7] = type_and_level[1];
    leader
}

/// A book with most of what the formats read.
fn book() -> Record {
    Record {
        leader: leader(*b"am"),
        fields: vec![
            control(*b"001", "12"),
            control(*b"003", "BR-PaBM"),
            control(*b"005", "20261126101500.0"),
            control(*b"008", "261126s2009    bl            000 0 por d"),
            data(*b"020", *b"  ", &[(b'a', "9788535914849")]),
            data(*b"040", *b"  ", &[(b'a', "BR-PaBM"), (b'b', "por")]),
            data(*b"082", *b"04", &[(b'a', "869.3"), (b'2', "22")]),
            data(
                *b"100",
                *b"1 ",
                &[
                    (b'a', "Lemos, Ana Maria."),
                    (b'd', "1950-"),
                    (b'e', "autora"),
                ],
            ),
            data(
                *b"245",
                *b"14",
                &[
                    (b'a', "The rio /"),
                    (b'b', "um romance :"),
                    (b'c', "Ana Maria Lemos."),
                ],
            ),
            data(*b"250", *b"  ", &[(b'a', "2. ed.")]),
            data(
                *b"264",
                *b" 1",
                &[
                    (b'a', "São Paulo :"),
                    (b'b', "Editora {Exemplo},"),
                    (b'c', "c2009."),
                ],
            ),
            data(*b"300", *b"  ", &[(b'a', "320 p. ;"), (b'c', "21 cm")]),
            data(*b"490", *b"0 ", &[(b'a', "Coleção Rio ;"), (b'v', "3")]),
            data(*b"500", *b"  ", &[(b'a', "Inclui índice.")]),
            data(*b"520", *b"  ", &[(b'a', "A história de um rio.")]),
            data(*b"650", *b" 4", &[(b'a', "Rios."), (b'z', "Paraná.")]),
            data(*b"650", *b" 7", &[(b'a', "Rios"), (b'2', "larpcal")]),
            data(*b"700", *b"1 ", &[(b'a', "Souza, J. P."), (b'4', "ill")]),
            data(*b"856", *b"40", &[(b'u', "https://example.org/rio")]),
        ],
    }
}

fn cited(record: &Record, deposit: Option<&str>, format: CitationFormat) -> String {
    cite(record, InstanceId(12), deposit, format)
}

#[test]
fn formats_are_named_by_the_parameter() {
    assert_eq!(CitationFormat::of("ris"), Some(CitationFormat::Ris));
    assert_eq!(CitationFormat::of("bibtex"), Some(CitationFormat::Bibtex));
    assert_eq!(CitationFormat::of("mods"), Some(CitationFormat::Mods));
    assert_eq!(CitationFormat::of("dc"), Some(CitationFormat::DublinCore));
    assert_eq!(CitationFormat::of("endnote"), None);
    assert_eq!(CitationFormat::Bibtex.file_suffix(), ".bib");
}

#[test]
fn ris_follows_kohas_lines() {
    let ris = cited(
        &book(),
        Some("https://b.example/repository/ab"),
        CitationFormat::Ris,
    );
    assert_eq!(
        ris,
        "TY  - BOOK\r\n\
         AU  - Lemos,Ana Maria\r\n\
         AU  - Souza,J.P.\r\n\
         TI  - The rio: um romance\r\n\
         T2  - Coleção Rio\r\n\
         SN  - 9788535914849\r\n\
         U1  - 869.3 22\r\n\
         PY  - 2009///\r\n\
         CY  - São Paulo\r\n\
         PB  - Editora {Exemplo}\r\n\
         KW  - Rios\r\n\
         KW  - Paraná\r\n\
         KW  - larpcal\r\n\
         N1  - Inclui índice\r\n\
         N2  - A história de um rio\r\n\
         UR  - https://example.org/rio\r\n\
         UR  - https://b.example/repository/ab\r\n\
         ER  - \r\n"
    );
}

#[test]
fn ris_types_dates_and_empty_lines() {
    let mut record = Record {
        leader: leader(*b"ax"),
        fields: vec![
            data(*b"110", *b"2 ", &[(b'b', "sem nome")]),
            data(*b"260", *b"  ", &[(b'c', "[199-?]"), (b'c', "s.d.")]),
        ],
    };
    let ris = cited(&record, None, CitationFormat::Ris);
    // No TY from the level, no ED without $a, a PY for each date.
    assert_eq!(
        ris,
        "TY  - GEN\r\nPY  - 1990///?]\r\nPY  - 0000///s.d\r\nER  - \r\n"
    );
    for (kind, tag) in [
        (*b"as", "SER"),
        (*b"em", "MAP"),
        (*b"jm", "SOUND"),
        (*b"tm", "MANSCPT"),
        (*b"zm", "GEN"),
    ] {
        record.leader = leader(kind);
        assert!(
            cited(&record, None, CitationFormat::Ris).starts_with(&format!("TY  - {tag}\r\n")),
            "{kind:?}"
        );
    }
}

#[test]
fn bibtex_reads_one_of_each_field() {
    let bibtex = cited(
        &book(),
        Some("https://b.example/repository/ab"),
        CitationFormat::Bibtex,
    );
    assert_eq!(
        bibtex,
        "@book{12,\n\
         \tauthor = {Lemos, Ana Maria. and Souza, J. P.},\n\
         \ttitle = {The rio /},\n\
         \tpublisher = {Editora \\{Exemplo\\},},\n\
         \tyear = {c2009.},\n\
         \tseries = {Coleção Rio ;},\n\
         \taddress = {São Paulo :},\n\
         \tedition = {2. ed.},\n\
         \tnote = {Inclui índice.},\n\
         \turl = {https://example.org/rio}\n\
         }\n"
    );
    let bare = Record {
        leader: leader(*b"am"),
        fields: vec![data(*b"245", *b"00", &[(b'a', "Só título")])],
    };
    assert_eq!(
        cited(
            &bare,
            Some("https://b.example/repository/ab"),
            CitationFormat::Bibtex
        ),
        "@book{12,\n\ttitle = {Só título},\n\turl = {https://b.example/repository/ab}\n}\n"
    );
}

#[test]
fn mods_names_titles_origin_and_subjects() {
    let mods = cited(
        &book(),
        Some("https://b.example/repository/ab"),
        CitationFormat::Mods,
    );
    for expected in [
        "<mods xmlns=\"http://www.loc.gov/mods/v3\"",
        "<nonSort>The </nonSort>",
        "<title>rio</title>",
        "<subTitle>um romance</subTitle>",
        "<name type=\"personal\">\n    <namePart>Lemos, Ana Maria.</namePart>",
        "<namePart type=\"date\">1950-</namePart>",
        "<roleTerm authority=\"marcrelator\" type=\"text\">creator</roleTerm>",
        "<roleTerm type=\"text\">autora</roleTerm>",
        "<roleTerm authority=\"marcrelator\" type=\"code\">ill</roleTerm>",
        "<typeOfResource>text</typeOfResource>",
        "<placeTerm type=\"code\" authority=\"marccountry\">bl</placeTerm>",
        "<placeTerm type=\"text\">São Paulo</placeTerm>",
        "<publisher>Editora {Exemplo}</publisher>",
        "<dateIssued>c2009</dateIssued>",
        "<dateIssued encoding=\"marc\">2009</dateIssued>",
        "<edition>2. ed.</edition>",
        "<issuance>monographic</issuance>",
        "<languageTerm authority=\"iso639-2b\" type=\"code\">por</languageTerm>",
        "<extent>320 p. ; 21 cm</extent>",
        "<abstract>A história de um rio.</abstract>",
        "<note type=\"statement of responsibility\">Ana Maria Lemos.</note>",
        "<note>Inclui índice.</note>",
        "<subject>\n    <topic>Rios</topic>\n    <geographic>Paraná</geographic>\n  </subject>",
        "<subject authority=\"larpcal\">\n    <topic>Rios</topic>",
        "<classification authority=\"ddc\" edition=\"22\">869.3</classification>",
        "<relatedItem type=\"series\">\n    <titleInfo>\n      <title>Coleção Rio ; 3</title>",
        "<identifier type=\"isbn\">9788535914849</identifier>",
        "<identifier type=\"uri\">https://example.org/rio</identifier>",
        "<url>https://example.org/rio</url>",
        "<url>https://b.example/repository/ab</url>",
        "<recordContentSource authority=\"marcorg\">BR-PaBM</recordContentSource>",
        "<recordCreationDate encoding=\"marc\">261126</recordCreationDate>",
        "<recordIdentifier source=\"BR-PaBM\">12</recordIdentifier>",
    ] {
        assert!(mods.contains(expected), "{expected}\n{mods}");
    }
    assert!(mods.ends_with("</mods>\n"));
}

#[test]
fn mods_escapes_and_leaves_out_what_is_empty() {
    let record = Record {
        leader: leader(*b"am"),
        fields: vec![data(*b"245", *b"00", &[(b'a', "Rios & <mares>.")])],
    };
    let mods = cited(&record, None, CitationFormat::Mods);
    assert!(
        mods.contains("<title>Rios &amp; &lt;mares&gt;</title>"),
        "{mods}"
    );
    assert!(!mods.contains("<name"));
    assert!(!mods.contains("<location>"));
}

#[test]
fn dublin_core_is_the_oai_crosswalk_with_the_deposit() {
    let dc = cited(
        &book(),
        Some("https://b.example/repository/ab"),
        CitationFormat::DublinCore,
    );
    assert!(dc.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<oai_dc:dc "));
    assert!(
        dc.contains("<dc:title>The rio / um romance :</dc:title>"),
        "{dc}"
    );
    assert!(dc.contains("<dc:identifier>URN:ISBN:9788535914849</dc:identifier>"));
    assert!(
        dc.contains("<dc:identifier>https://b.example/repository/ab</dc:identifier></oai_dc:dc>")
    );
}

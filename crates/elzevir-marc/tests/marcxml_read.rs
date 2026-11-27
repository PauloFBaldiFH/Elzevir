// SPDX-License-Identifier: GPL-3.0-or-later
//! Streaming MARCXML, source fidelity and bounded hostile input.
#![expect(
    clippy::unwrap_used,
    reason = "test helpers must fail on malformed fixtures"
)]

use std::io::{self, BufRead, Cursor, Read};

use elzevir_marc::{Field, MarcXmlReader, Record, XmlLimits, XmlProblem};

const LEADER: &str = "00000nam a2200000 a 4500";

fn document(fields: &str) -> String {
    format!(
        "<record xmlns='http://www.loc.gov/MARC21/slim'><leader>{LEADER}</leader>{fields}</record>"
    )
}

fn records(xml: &str) -> Vec<Record> {
    MarcXmlReader::new(xml.as_bytes(), XmlLimits::default())
        .collect::<Result<_, _>>()
        .unwrap()
}

#[test]
fn fields_indicators_repetitions_entities_and_cdata_keep_their_values() {
    let xml = document(
        "<controlfield tag='001'> 0001 </controlfield><controlfield tag='001'></controlfield><datafield tag='245' ind1='1' ind2=' '><subfield code='a'> A &amp; B &#xE9; <![CDATA[<raw>]]><!-- split --> </subfield><subfield code='a'/></datafield><datafield tag='999' ind1=' ' ind2=' '><subfield code='c'>42</subfield><subfield code='d'>43</subfield></datafield><datafield tag='952' ind1=' ' ind2=' '><subfield code='9'>44</subfield></datafield>",
    );
    let parsed = records(&xml);
    assert_eq!(parsed.len(), 1);
    let record = parsed.first().unwrap();
    assert_eq!(record.fields.len(), 5);
    let Field::Control { value, .. } = record.fields.first().unwrap() else {
        panic!("unexpected field kind")
    };
    assert_eq!(value, b" 0001 ");
    let Field::Data {
        indicators,
        subfields,
        ..
    } = record.fields.get(2).unwrap()
    else {
        panic!("unexpected field kind")
    };
    assert_eq!(indicators, b"1 ");
    assert_eq!(subfields.len(), 2);
    assert_eq!(
        subfields.first().unwrap().value,
        " A & B é <raw> ".as_bytes()
    );
    assert!(subfields.get(1).unwrap().value.is_empty());
    let Field::Data { subfields, .. } = record.fields.get(3).unwrap() else {
        panic!("unexpected field kind")
    };
    assert_eq!(
        subfields
            .iter()
            .map(|s| (s.code, s.value.as_slice()))
            .collect::<Vec<_>>(),
        [(b'c', b"42".as_slice()), (b'd', b"43")]
    );
    let Field::Data { subfields, .. } = record.fields.get(4).unwrap() else {
        panic!("unexpected field kind")
    };
    assert_eq!(subfields.first().unwrap().code, b'9');
    assert_eq!(subfields.first().unwrap().value, b"44");
}

#[test]
fn namespaced_collections_stream_one_record_at_a_time_with_offsets() {
    let first = format!("<m:record><m:leader>{LEADER}</m:leader></m:record>");
    let header = "<?xml version='1.0' encoding='UTF-8'?><m:collection xmlns:m='http://www.loc.gov/MARC21/slim'>";
    let xml = format!("{header}{first}{first}</m:collection>");
    let mut source = Cursor::new(xml.as_bytes());
    {
        let mut reader = MarcXmlReader::new(&mut source, XmlLimits::default());
        assert!(reader.next().unwrap().is_ok());
        assert_eq!(reader.record_offset(), u64::try_from(header.len()).unwrap());
    }
    assert_eq!(
        source.position(),
        u64::try_from(header.len() + first.len()).unwrap()
    );
    assert_eq!(records(&xml).len(), 2);
    assert_eq!(records(&format!("\u{feff}{xml}")).len(), 2);
    assert!(records("<collection xmlns='http://www.loc.gov/MARC21/slim'/>").is_empty());
}

#[test]
fn io_errors_are_terminal_without_exposing_the_underlying_payload() {
    struct Failed;
    impl Read for Failed {
        fn read(&mut self, _out: &mut [u8]) -> io::Result<usize> {
            Err(io::Error::other("SYNTHETIC_PRIVATE_SENTINEL"))
        }
    }
    impl BufRead for Failed {
        fn fill_buf(&mut self) -> io::Result<&[u8]> {
            Err(io::Error::other("SYNTHETIC_PRIVATE_SENTINEL"))
        }
        fn consume(&mut self, _amount: usize) {}
    }
    let mut reader = MarcXmlReader::new(Failed, XmlLimits::default());
    let error = reader.next().unwrap().unwrap_err();
    assert_eq!(error.problem, XmlProblem::Input);
    assert!(!error.to_string().contains("SYNTHETIC_PRIVATE_SENTINEL"));
    assert!(!format!("{error:?}").contains("SYNTHETIC_PRIVATE_SENTINEL"));
    assert!(reader.next().is_none());
}

#[test]
fn xml_text_is_utf8_even_when_the_marc_leader_was_blank() {
    let xml = document("<controlfield tag='001'>café</controlfield>").replace("nam a", "nam  ");
    let parsed = records(&xml);
    assert_eq!(parsed.first().unwrap().leader.get(9), Some(&b'a'));
    let Field::Control { value, .. } = parsed.first().unwrap().fields.first().unwrap() else {
        panic!("unexpected field kind")
    };
    assert_eq!(value, "café".as_bytes());
}

#[test]
fn a_blank_subfield_code_is_kept_as_koha_keeps_it() {
    // Found in a real Koha catalog: `500 $a` empty followed by a subfield coded with a space.
    let xml = document(
        "<datafield tag='500' ind1=' ' ind2=' '><subfield code='a'></subfield><subfield code=' '>- 19166</subfield></datafield>",
    );
    let record = records(&xml).remove(0);
    let Some(Field::Data { subfields, .. }) = record.fields.first() else {
        panic!("one data field");
    };
    let codes: Vec<(u8, &[u8])> = subfields
        .iter()
        .map(|subfield| (subfield.code, subfield.value.as_slice()))
        .collect();
    assert_eq!(codes, vec![(b'a', &b""[..]), (b' ', &b"- 19166"[..])]);
    let mut written = String::new();
    record.write_marcxml_document(&mut written).unwrap();
    assert!(
        written.contains("<subfield code=\" \">- 19166</subfield>"),
        "{written}"
    );
    assert_eq!(records(&written), vec![record]);
}

#[test]
fn malformed_structure_and_missing_attributes_are_refused_without_echoing_input() {
    for xml in [
        document("<datafield tag='245' ind2='0'/>"),
        document("<controlfield tag='245'>SYNTHETIC_PRIVATE_SENTINEL</controlfield>"),
        document("<controlfield tag='000'/>"),
        document("<datafield tag='001' ind1=' ' ind2=' '/>"),
        document("<datafield tag='245' tag='246' ind1=' ' ind2=' '/>"),
        document("<datafield tag='245' ind1='é' ind2=' '/>"),
        document("<datafield tag='245' ind1=' ' ind2=' '><subfield code='é'/></datafield>"),
        document("<datafield tag='245' ind1=' ' ind2=' '><subfield/></datafield>"),
        document("<leader>SYNTHETIC_PRIVATE_SENTINEL</leader>"),
        document("<subfield code='a'>SYNTHETIC_PRIVATE_SENTINEL</subfield>"),
        "<record xmlns='http://www.loc.gov/MARC21/slim'/>".to_owned(),
        document("").replace("</leader>", "</wrong>"),
    ] {
        let mut reader = MarcXmlReader::new(xml.as_bytes(), XmlLimits::default());
        let error = reader.next().unwrap().unwrap_err();
        assert!(!error.to_string().contains("SYNTHETIC_PRIVATE_SENTINEL"));
        assert!(!format!("{error:?}").contains("SYNTHETIC_PRIVATE_SENTINEL"));
        assert!(reader.next().is_none());
    }
}

#[test]
fn dtd_entities_foreign_namespaces_authorities_and_non_utf8_are_rejected() {
    for xml in [
        format!(
            "<!DOCTYPE record SYSTEM 'https://example.invalid/SYNTHETIC_PRIVATE_SENTINEL'>{}",
            document("")
        ),
        document("<controlfield tag='001'>&SYNTHETIC_PRIVATE_SENTINEL;</controlfield>"),
        document("").replace("MARC21/slim", "SYNTHETIC_PRIVATE_SENTINEL"),
        document("<controlfield xmlns='urn:foreign' tag='001'/>"),
        document("").replace("<record ", "<record type='Authority' "),
        document("").replace("nam a", "nzm a"),
        format!(
            "<?xml version='1.0' encoding='ISO-8859-1'?>{}",
            document("")
        ),
        document("<controlfield tag='001'>&#x1f;</controlfield>"),
        document("<!-- invalid \0 comment -->"),
        document("<?ignored invalid \0 instruction?>"),
    ] {
        assert!(
            MarcXmlReader::new(xml.as_bytes(), XmlLimits::default())
                .collect::<Result<Vec<_>, _>>()
                .is_err(),
            "{xml}"
        );
    }
    for fields in [
        "<controlfield tag='001'>UTF8_MARKER</controlfield>",
        "<!-- UTF8_MARKER -->",
    ] {
        let mut invalid = document(fields).into_bytes();
        let position = invalid
            .windows(11)
            .position(|bytes| bytes == b"UTF8_MARKER")
            .unwrap();
        invalid[position] = 0xff;
        assert!(
            MarcXmlReader::new(invalid.as_slice(), XmlLimits::default())
                .collect::<Result<Vec<_>, _>>()
                .is_err()
        );
    }
}

#[test]
fn a_huge_event_is_stopped_while_reading_instead_of_after_buffering() {
    let xml = format!(
        "<record xmlns='http://www.loc.gov/MARC21/slim' ignored='{}' />",
        "x".repeat(100_000)
    );
    let mut source = Cursor::new(xml.as_bytes());
    {
        let limits = XmlLimits {
            max_element_bytes: 64,
            ..XmlLimits::default()
        };
        let mut reader = MarcXmlReader::new(&mut source, limits);
        assert!(matches!(
            reader.next().unwrap().unwrap_err().problem,
            XmlProblem::Limit(_)
        ));
    }
    assert!(source.position() <= 65);
}

#[test]
fn depth_record_bytes_element_count_and_total_bytes_are_bounded() {
    let xml = document(
        "<datafield tag='245' ind1=' ' ind2=' '><subfield code='a'>Synthetic</subfield></datafield>",
    );
    for limits in [
        XmlLimits {
            max_depth: 2,
            ..XmlLimits::default()
        },
        XmlLimits {
            max_record_bytes: 70,
            ..XmlLimits::default()
        },
        XmlLimits {
            max_elements_per_record: 3,
            ..XmlLimits::default()
        },
        XmlLimits {
            max_input_bytes: xml.len() - 1,
            ..XmlLimits::default()
        },
    ] {
        let result = MarcXmlReader::new(xml.as_bytes(), limits).collect::<Result<Vec<_>, _>>();
        assert!(matches!(result.unwrap_err().problem, XmlProblem::Limit(_)));
    }
}

#[test]
fn truncation_second_roots_and_too_many_records_are_terminal() {
    let xml = document("");
    for end in 0..xml.len() {
        let bytes = xml.as_bytes().get(..end).unwrap();
        assert!(
            MarcXmlReader::new(bytes, XmlLimits::default())
                .collect::<Result<Vec<_>, _>>()
                .is_err()
        );
    }
    assert!(
        MarcXmlReader::new(format!("{xml}{xml}").as_bytes(), XmlLimits::default())
            .collect::<Result<Vec<_>, _>>()
            .is_err()
    );
    let record = format!("<record><leader>{LEADER}</leader></record>");
    let xml =
        format!("<collection xmlns='http://www.loc.gov/MARC21/slim'>{record}{record}</collection>");
    let limits = XmlLimits {
        max_records: 1,
        ..XmlLimits::default()
    };
    let mut reader = MarcXmlReader::new(xml.as_bytes(), limits);
    assert!(reader.next().unwrap().is_ok());
    assert!(matches!(
        reader.next().unwrap().unwrap_err().problem,
        XmlProblem::Limit(_)
    ));
    assert!(reader.next().is_none());
}

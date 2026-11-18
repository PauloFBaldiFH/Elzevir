// SPDX-License-Identifier: GPL-3.0-or-later
//! `MarcEdit` syntax, byte preservation and streaming limits.
use std::io::{self, BufReader, Cursor, Read};

use elzevir_marc::{Field, MarcTextReader, MrkLimits, MrkProblem, Subfield, Tag};

const LEADER: &str = "00000nam a2200000 a 4500";
const LEADER_LINE: &str = "=LDR  00000nam a2200000 a 4500\n";
const FIELD_LINE: &str = "=245  10$aSynthetic title\n";

#[test]
fn decodes_mnemonics_after_splitting_and_preserves_utf8_spaces_and_order() {
    let input = format!(
        "\u{feff}=LDR  {}\r\n=001  A\\B{{bsol}}{{dollar}}\r\n\
         =245  \\0$a  Título {{dollar}}5 {{lcub}}dollar{{rcub}}  $a$aother {{bsol}} \\\t\r\n\
         =245   0$aRepeat\r\n\r\n{LEADER_LINE}{FIELD_LINE}",
        LEADER.replace(' ', "\\")
    );
    let mut reader = MarcTextReader::new(
        BufReader::with_capacity(3, input.as_bytes()),
        MrkLimits::default(),
    );
    let first = reader.next().unwrap().unwrap();
    assert_eq!(reader.record_offset(), 0);
    assert_eq!(&first.leader, LEADER.as_bytes());
    assert_eq!(
        first.fields,
        vec![
            Field::Control {
                tag: Tag::new(*b"001"),
                value: b"A B\\$".to_vec(),
            },
            Field::Data {
                tag: Tag::new(*b"245"),
                indicators: *b" 0",
                subfields: vec![
                    Subfield {
                        code: b'a',
                        value: "  Título $5 {dollar}  ".as_bytes().to_vec(),
                    },
                    Subfield {
                        code: b'a',
                        value: Vec::new(),
                    },
                    Subfield {
                        code: b'a',
                        value: b"other \\ \\\t".to_vec(),
                    },
                ],
            },
            Field::Data {
                tag: Tag::new(*b"245"),
                indicators: *b" 0",
                subfields: vec![Subfield {
                    code: b'a',
                    value: b"Repeat".to_vec(),
                }],
            },
        ]
    );
    let second = reader.next().unwrap().unwrap();
    assert_eq!(second.fields.len(), 1);
    assert_eq!(reader.record_offset(), input.rfind("=LDR").unwrap() as u64);
    assert!(reader.next().is_none());
}

#[test]
fn empty_inputs_and_last_lines_without_newline_are_supported() {
    for input in ["", "\n\r\n\n", "\u{feff}", "\u{feff}\r\n"] {
        let mut reader = MarcTextReader::new(input.as_bytes(), MrkLimits::default());
        assert!(reader.next().is_none(), "{input:?}");
        assert!(reader.next().is_none());
    }
    let input = format!("{LEADER_LINE}=001  LAST");
    let record = MarcTextReader::new(input.as_bytes(), MrkLimits::default())
        .next()
        .unwrap()
        .unwrap();
    assert_eq!(record.fields.len(), 1);

    let input = format!(
        "{}\n{FIELD_LINE}",
        LEADER_LINE.replace("nam a", "nam  ").trim_end()
    );
    let record = MarcTextReader::new(input.as_bytes(), MrkLimits::default())
        .next()
        .unwrap()
        .unwrap();
    assert_eq!(record.leader.get(9), Some(&b'a'));
}

#[test]
fn malformed_records_are_terminal_and_diagnostics_do_not_echo_values() {
    for line in [
        b"=001 test".as_slice(),
        b"=245  10$a{secret}",
        b"=245  10$a{dollar",
        b"=245  10$a}",
        b"=245  10$?text",
        b"=245  10$a$",
        b"=245  10prefix$a",
        b"=245  10$a\xff",
        b"=001  prefix\x1fsuffix",
        b"=001  prefix\rsuffix",
        b"=245  \xc3\xa9$a",
        b"second physical line of a field",
        LEADER_LINE.trim_end().as_bytes(),
    ] {
        let first = format!("{LEADER_LINE}{FIELD_LINE}\n");
        let input = [first.as_bytes(), LEADER_LINE.as_bytes(), line, b"\n"].concat();
        let mut reader = MarcTextReader::new(input.as_slice(), MrkLimits::default());
        assert!(reader.next().unwrap().is_ok());
        let error = reader.next().unwrap().unwrap_err();
        assert!(matches!(error.problem, MrkProblem::Invalid(_)));
        assert_eq!((error.index, error.line), (1, 5));
        assert_eq!(error.offset, (first.len() + LEADER_LINE.len()) as u64);
        assert!(!error.to_string().contains("secret"));
        assert!(reader.next().is_none());
    }
}

#[test]
fn leader_is_required_and_authority_input_is_refused() {
    for input in [
        FIELD_LINE.to_owned(),
        "=LDR  short\n".into(),
        LEADER_LINE.replace("nam", "nz "),
    ] {
        let error = MarcTextReader::new(input.as_bytes(), MrkLimits::default())
            .next()
            .unwrap()
            .unwrap_err();
        assert!(matches!(error.problem, MrkProblem::Invalid(_)));
    }
}

#[test]
fn exact_limits_allow_the_record_and_its_separator() {
    let input = format!("{LEADER_LINE}{FIELD_LINE}\r\n");
    let limits = MrkLimits {
        max_line_bytes: LEADER_LINE.len().max(FIELD_LINE.len()),
        max_record_bytes: LEADER_LINE.len() + FIELD_LINE.len(),
        max_lines_per_record: 2,
        max_input_bytes: input.len(),
        max_records: 1,
    };
    let mut reader = MarcTextReader::new(input.as_bytes(), limits);
    assert!(reader.next().unwrap().is_ok());
    assert!(reader.next().is_none());
}

#[test]
fn rejects_each_limit_and_only_consumes_a_bounded_prefix_of_a_long_line() {
    let oversized = format!("{LEADER_LINE}=001  {}\n", "x".repeat(100_000));
    let defaults = MrkLimits::default();
    for (limits, label, max_consumed) in [
        (
            MrkLimits {
                max_line_bytes: 32,
                ..defaults
            },
            "line bytes",
            LEADER_LINE.len() + 33,
        ),
        (
            MrkLimits {
                max_record_bytes: 40,
                ..defaults
            },
            "record bytes",
            41,
        ),
        (
            MrkLimits {
                max_lines_per_record: 1,
                ..defaults
            },
            "lines per record",
            LEADER_LINE.len() + 3,
        ),
        (
            MrkLimits {
                max_input_bytes: 40,
                ..defaults
            },
            "input bytes",
            41,
        ),
        (
            MrkLimits {
                max_records: 0,
                ..defaults
            },
            "record count",
            6,
        ),
    ] {
        let mut input = Cursor::new(oversized.as_bytes());
        let error = MarcTextReader::new(&mut input, limits)
            .next()
            .unwrap()
            .unwrap_err();
        assert_eq!(error.problem, MrkProblem::Limit(label));
        assert!(input.position() <= max_consumed as u64, "{label}");
    }
}

#[test]
fn yields_a_complete_record_before_reading_the_next_one() {
    struct Fails;
    impl Read for Fails {
        fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
            Err(io::Error::other("private input path"))
        }
    }
    let first = format!("{LEADER_LINE}{FIELD_LINE}\n");
    let input = BufReader::with_capacity(4, first.as_bytes().chain(Fails));
    let mut reader = MarcTextReader::new(input, MrkLimits::default());
    assert!(reader.next().unwrap().is_ok());
    let error = reader.next().unwrap().unwrap_err();
    assert_eq!(error.problem, MrkProblem::Input);
    assert_eq!(
        (error.index, error.line, error.offset),
        (1, 4, first.len() as u64)
    );
    assert!(!error.to_string().contains("private input path"));
    assert!(reader.next().is_none());
}

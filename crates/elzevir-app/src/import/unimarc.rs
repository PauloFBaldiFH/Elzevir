// SPDX-License-Identifier: GPL-3.0-or-later
//! A bibliographic UNIMARC projection. The unmodified ISO record remains available
//! even for fields without a MARC21 equivalent.
use super::RecordFailure;
use crate::catalog::{NewRecord, OriginalRecord};
use crate::marc_mapping::marc_to_bibframe;
use elzevir_marc::{Field, Record, Subfield, Tag, iso5426_to_utf8};

pub(super) fn recognizes(record: &Record) -> bool {
    record.fields.iter().any(|f| f.tag().as_bytes() == b"200")
        && !record.fields.iter().any(|f| f.tag().as_bytes() == b"245")
}

pub(super) fn map(mut record: Record, bytes: Vec<u8>) -> Result<NewRecord, RecordFailure> {
    let (declared, decoded) = decode(&mut record)?;
    let mut mapped = Record {
        leader: record.leader,
        fields: Vec::new(),
    };
    mapped.leader[9] = b'a';
    for field in record.fields {
        let Field::Data {
            tag,
            indicators,
            subfields,
        } = field
        else {
            if field.tag().as_bytes() == b"001" {
                mapped.fields.push(field);
            }
            continue;
        };
        let Some((target, codes)) = field_map(*tag.as_bytes(), indicators) else {
            continue;
        };
        let subject = target[0] == b'6';
        let mut values = Vec::new();
        for sub in subfields {
            let code = match sub.code {
                b'3' if subject
                    || matches!(target, b"100" | b"700" | b"110" | b"710" | b"111" | b"711") =>
                {
                    b'0'
                }
                b'x' if subject => b'x',
                b'y' if subject => b'z',
                b'z' if subject => b'y',
                b'j' if subject => b'v',
                b'2' if subject => b'2',
                b'0' | b'1' => sub.code,
                c => match codes.iter().find(|(from, _)| *from == c) {
                    Some((_, to)) => *to,
                    None => continue,
                },
            };
            let value = sub.value;
            // Inverted personal names carry the remainder of the name in UNIMARC $b.
            if code == b'c'
                && sub.code == b'b'
                && matches!(target, b"100" | b"700" | b"600")
                && let Some(name) = values
                    .iter_mut()
                    .rev()
                    .find(|s: &&mut Subfield| s.code == b'a')
            {
                name.value.extend_from_slice(b", ");
                name.value.extend_from_slice(&value);
                continue;
            }
            values.push(Subfield { code, value });
        }
        mapped.fields.push(Field::Data {
            tag: Tag::new(*target),
            indicators: projection_indicators(*tag.as_bytes(), *target, indicators, &values),
            subfields: values,
        });
    }
    let mut normalized = Vec::new();
    mapped
        .write_iso2709(&mut normalized)
        .map_err(RecordFailure::Source)?;
    let mut model = marc_to_bibframe(&mapped, Some(normalized)).map_err(RecordFailure::Mapping)?;
    model.instance.original = Some(OriginalRecord {
        serialization: "ISO 2709".into(),
        bytes,
        declared,
        decoded,
        family: "UNIMARC".into(),
    });
    Ok(model)
}

type FieldMap = (&'static [u8; 3], &'static [(u8, u8)]);
const PERSON: &[(u8, u8)] = &[
    (b'a', b'a'),
    (b'b', b'c'),
    (b'c', b'c'),
    (b'f', b'd'),
    (b'g', b'q'),
    (b'4', b'4'),
];
const CORPORATE: &[(u8, u8)] = &[(b'a', b'a'), (b'b', b'b'), (b'c', b'c'), (b'4', b'4')];
const MEETING: &[(u8, u8)] = &[
    (b'a', b'a'),
    (b'd', b'n'),
    (b'e', b'c'),
    (b'f', b'd'),
    (b'4', b'4'),
];
const TERM: &[(u8, u8)] = &[(b'a', b'a')];

fn field_map(tag: [u8; 3], indicators: [u8; 2]) -> Option<FieldMap> {
    Some(match &tag {
        b"200" => (
            b"245",
            &[
                (b'a', b'a'),
                (b'e', b'b'),
                (b'h', b'n'),
                (b'i', b'p'),
                (b'f', b'c'),
                (b'g', b'c'),
            ],
        ),
        b"010" => (b"020", &[(b'a', b'a'), (b'b', b'q')]),
        b"011" => (b"022", TERM),
        b"101" => (b"041", &[(b'a', b'a'), (b'c', b'h')]),
        b"205" => (b"250", TERM),
        b"210" => (b"260", &[(b'a', b'a'), (b'c', b'b'), (b'd', b'c')]),
        b"215" => (
            b"300",
            &[(b'a', b'a'), (b'c', b'b'), (b'd', b'c'), (b'e', b'e')],
        ),
        b"225" => (b"490", &[(b'a', b'a'), (b'v', b'v')]),
        b"300" => (b"500", TERM),
        b"316" | b"317" => (b"590", &[(b'a', b'a'), (b'5', b'5')]),
        b"600" | b"602" => (b"600", PERSON),
        b"601" if indicators[0] == b'1' => (b"611", MEETING),
        b"601" => (b"610", CORPORATE),
        b"605" => (b"630", TERM),
        b"606" => (b"650", TERM),
        b"607" => (b"651", TERM),
        b"608" => (b"655", TERM),
        b"700" | b"720" => (b"100", PERSON),
        b"701" | b"702" | b"721" | b"722" => (b"700", PERSON),
        b"710" if indicators[0] == b'1' => (b"111", MEETING),
        b"711" | b"712" if indicators[0] == b'1' => (b"711", MEETING),
        b"710" => (b"110", CORPORATE),
        b"711" | b"712" => (b"710", CORPORATE),
        _ => return None,
    })
}

fn projection_indicators(
    source: [u8; 3],
    target: [u8; 3],
    old: [u8; 2],
    values: &[Subfield],
) -> [u8; 2] {
    if target == *b"245" {
        return [if old[0] == b'1' { b'1' } else { b'0' }, b'0'];
    }
    let first = match &source {
        b"602" | b"720" | b"721" | b"722" => b'3',
        b"600" | b"700" | b"701" | b"702" => {
            if old[1] == b'0' {
                b'0'
            } else {
                b'1'
            }
        }
        b"601" | b"710" | b"711" | b"712" => b'2',
        _ => b' ',
    };
    let second = if target[0] == b'6' {
        if values.iter().any(|s| s.code == b'2') {
            b'7'
        } else {
            b'4'
        }
    } else {
        b' '
    };
    [first, second]
}

pub(super) fn decode(record: &mut Record) -> Result<(String, String), RecordFailure> {
    let code = record.fields.iter().find_map(|f| match f {
        Field::Data { tag, subfields, .. } if tag.as_bytes() == b"100" => {
            subfields.iter().find(|s| s.code == b'a')?.value.get(26..30)
        }
        _ => None,
    });
    let valid_utf8 = record.fields.iter().all(|f| match f {
        Field::Control { value, .. } => std::str::from_utf8(value).is_ok(),
        Field::Data { subfields, .. } => subfields
            .iter()
            .all(|s| std::str::from_utf8(&s.value).is_ok()),
    });
    let legacy = match code {
        Some(v)
            if v.chunks_exact(2)
                .all(|c| matches!(c, b"01" | b"03" | b"  "))
                && v.chunks_exact(2).any(|c| c == b"03") =>
        {
            true
        }
        Some(b"50  " | b"    ") | None if valid_utf8 => false,
        _ => return Err(RecordFailure::Encoding),
    };
    let declared = code.map_or_else(
        || "undeclared".into(),
        |v| String::from_utf8_lossy(v).into_owned(),
    );

    if legacy {
        for field in &mut record.fields {
            match field {
                Field::Control { value, .. } => {
                    *value = iso5426_to_utf8(value).into_owned().into_bytes();
                }
                Field::Data { subfields, .. } => {
                    for s in subfields {
                        s.value = iso5426_to_utf8(&s.value).into_owned().into_bytes();
                    }
                }
            }
        }
        declare_utf8(record);
    }
    record.leader[9] = b'a';
    Ok((declared, if legacy { "ISO 5426" } else { "UTF-8" }.into()))
}

pub(super) fn declare_utf8(record: &mut Record) {
    // Mark the working record as UTF-8. Keep the original declaration in evidence.
    for f in &mut record.fields {
        if let Field::Data { tag, subfields, .. } = f
            && tag.as_bytes() == b"100"
        {
            for s in subfields.iter_mut().filter(|s| s.code == b'a') {
                if let Some(code) = s.value.get_mut(26..30) {
                    code.copy_from_slice(b"50  ");
                }
            }
        }
    }
}

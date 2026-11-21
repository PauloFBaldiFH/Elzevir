// SPDX-License-Identifier: GPL-3.0-or-later
//! Join the two catalog tables and preserve equivalent fields from the MARC source.

use std::collections::{BTreeMap, BTreeSet};

use elzevir_marc::{Field, Limits, MarcXmlReader, Record, Subfield, Tag, XmlLimits};

use super::{Biblio, CatalogRows, DumpError, DumpProblem, Item, positive};
use crate::marc_mapping::items::cents;

const ITEM_TAG: Tag = Tag::new(*b"952");

pub(super) fn records(rows: CatalogRows) -> Result<Vec<super::MetadataRecord>, DumpError> {
    let mut records = Vec::with_capacity(rows.biblios.len());
    let mut bibs = BTreeMap::new();
    let mut primary = BTreeSet::new();
    let mut embedded = Embedded::default();
    for biblio in rows.biblios {
        let index = records.len();
        let at = biblio.offset;
        if bibs.insert(biblio.number, index).is_some()
            || biblio.primary_key.is_some_and(|id| !primary.insert(id))
        {
            return Err(error(at, DumpProblem::Duplicate("bibliographic key")));
        }
        let (record, declares_utf8) = xml(&biblio)?;
        embedded.index(index, at, &record)?;
        records.push(super::MetadataRecord {
            record,
            offset: at,
            xml: biblio.xml,
            declares_utf8,
        });
    }
    let mut extra = BTreeMap::<usize, Vec<Field>>::new();
    let (mut numbers, mut barcodes) = (BTreeSet::new(), BTreeSet::new());
    for item in rows.items {
        let at = item.offset;
        if !numbers.insert(item.number)
            || item
                .barcode()
                .is_some_and(|barcode| !barcodes.insert(barcode.to_owned()))
        {
            return Err(error(at, DumpProblem::Duplicate("itemnumber or barcode")));
        }
        let owner = *bibs
            .get(&item.biblio)
            .ok_or_else(|| error(at, DumpProblem::Orphan))?;
        let record = records
            .get(owner)
            .map(|r| &r.record)
            .ok_or_else(|| error(at, DumpProblem::Orphan))?;
        if embedded.has_equivalent(owner, record, &item)? {
            continue;
        }
        let append = extra.entry(owner).or_default();
        if record.fields.len() + append.len() >= Limits::default().max_fields {
            return Err(error(at, DumpProblem::Limit("MARC field count")));
        }
        append.push(item.field());
    }
    for (index, row) in records.iter_mut().enumerate() {
        if let Some(extra) = extra.remove(&index) {
            append_items(&mut row.record, extra);
        }
    }
    Ok(records)
}

fn error(offset: u64, problem: DumpProblem) -> DumpError {
    DumpError { offset, problem }
}

/// Koha: `Koha::Biblio::Metadata::record`.
fn xml(biblio: &Biblio) -> Result<(Record, bool), DumpError> {
    let invalid = || error(biblio.offset, DumpProblem::Metadata);
    let mut reader = MarcXmlReader::new(biblio.xml.as_slice(), XmlLimits::default());
    let mut record = reader.next().ok_or_else(invalid)?.map_err(|_| invalid())?;
    if reader.next().is_some() {
        return Err(invalid());
    }
    if record.fields.len() > Limits::default().max_fields {
        return Err(error(biblio.offset, DumpProblem::Limit("MARC field count")));
    }
    for field in &record.fields {
        if let Field::Data { tag, subfields, .. } = field
            && tag.as_bytes() == b"999"
            && subfields.iter().any(|subfield| {
                subfield.code == b'c' && positive(&subfield.value) != Some(biblio.number)
            })
        {
            return Err(error(
                biblio.offset,
                DumpProblem::Conflict("biblionumber / MARC 999 $c"),
            ));
        }
    }
    let number = Subfield {
        code: b'c',
        value: biblio.number.to_string().into_bytes(),
    };
    if let Some(Field::Data { subfields, .. }) = record
        .fields
        .iter_mut()
        .find(|f| f.tag().as_bytes() == b"999")
    {
        if !subfields.iter().any(|s| s.code == b'c') {
            subfields.push(number);
        }
    } else {
        record.fields.push(Field::Data {
            tag: Tag::new(*b"999"),
            indicators: *b"  ",
            subfields: vec![number],
        });
    }
    Ok((record, reader.declares_utf8()))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct EmbeddedPlace {
    owner: usize,
    field: usize,
}

#[derive(Default)]
struct Embedded {
    numbers: BTreeMap<u64, EmbeddedPlace>,
    barcodes: BTreeMap<String, EmbeddedPlace>,
    unkeyed: BTreeSet<usize>,
}

impl Embedded {
    fn index(&mut self, owner: usize, offset: u64, record: &Record) -> Result<(), DumpError> {
        for (field, data) in record.fields.iter().enumerate() {
            if data.tag() != ITEM_TAG {
                continue;
            }
            let place = EmbeddedPlace { owner, field };
            let number = embedded_value(data, b'9', offset)?
                .map(|value| {
                    positive(value).ok_or_else(|| error(offset, DumpProblem::Identifier("952 $9")))
                })
                .transpose()?;
            let barcode = embedded_value(data, b'p', offset)?.and_then(text);
            if number.is_none() && barcode.is_none() {
                self.unkeyed.insert(owner);
            }
            if number.is_some_and(|number| self.numbers.insert(number, place).is_some())
                || barcode.is_some_and(|barcode| {
                    self.barcodes.insert(barcode.to_owned(), place).is_some()
                })
            {
                return Err(error(
                    offset,
                    DumpProblem::Duplicate("embedded itemnumber or barcode"),
                ));
            }
        }
        Ok(())
    }

    fn has_equivalent(
        &self,
        owner: usize,
        record: &Record,
        item: &Item,
    ) -> Result<bool, DumpError> {
        let conflict = |reason| error(item.offset, DumpProblem::Conflict(reason));
        let id = self.numbers.get(&item.number);
        let barcode = item
            .barcode()
            .and_then(|barcode| self.barcodes.get(barcode));
        if id.zip(barcode).is_some_and(|(id, barcode)| id != barcode) {
            return Err(conflict(
                "itemnumber and barcode point to different MARC fields",
            ));
        }
        let Some(place) = id.or(barcode) else {
            if self.unkeyed.contains(&owner) {
                return Err(conflict("embedded item has neither itemnumber nor barcode"));
            }
            return Ok(false);
        };
        if place.owner != owner {
            return Err(conflict("item belongs to another biblionumber"));
        }
        let field = record
            .fields
            .get(place.field)
            .ok_or_else(|| conflict("embedded item"))?;
        if embedded_value(field, b'9', item.offset)?
            .is_some_and(|value| positive(value) != Some(item.number))
        {
            return Err(conflict("itemnumber differs"));
        }
        for (code, value) in &item.values {
            let old = embedded_value(field, *code, item.offset)?;
            if !equivalent(*code, old, value.as_deref()) {
                return Err(conflict("values differ for a declared item column"));
            }
        }
        Ok(true)
    }
}

fn embedded_value(field: &Field, code: u8, offset: u64) -> Result<Option<&[u8]>, DumpError> {
    let Field::Data { subfields, .. } = field else {
        return Ok(None);
    };
    let mut values = subfields.iter().filter(|subfield| subfield.code == code);
    let first = values.next().map(|subfield| subfield.value.as_slice());
    if values.next().is_some() {
        return Err(error(
            offset,
            DumpProblem::Conflict("repeated item subfield"),
        ));
    }
    Ok(first.filter(|value| !value.is_empty()))
}

fn equivalent(code: u8, old: Option<&[u8]>, new: Option<&[u8]>) -> bool {
    let old = old.and_then(text);
    let new = new.and_then(text);
    if old == new {
        return true;
    }
    if matches!(code, b'0' | b'1' | b'4' | b'5' | b'7') {
        let number = |value: Option<&str>| value.unwrap_or("0").parse::<i64>().ok();
        return number(old)
            .zip(number(new))
            .is_some_and(|(old, new)| old == new);
    }
    if matches!(code, b'g' | b'v') {
        return old
            .and_then(cents)
            .zip(new.and_then(cents))
            .is_some_and(|(old, new)| old == new);
    }
    false
}

fn text(bytes: &[u8]) -> Option<&str> {
    let value = std::str::from_utf8(bytes).ok()?.trim();
    (!value.is_empty()).then_some(value)
}

impl Item {
    fn barcode(&self) -> Option<&str> {
        self.values
            .iter()
            .find(|(code, _)| *code == b'p')
            .and_then(|(_, value)| value.as_deref())
            .and_then(text)
    }

    fn field(self) -> Field {
        let mut subfields: Vec<_> = self
            .values
            .into_iter()
            .filter_map(|(code, value)| {
                Some(Subfield {
                    code,
                    value: value?,
                })
            })
            .collect();
        subfields.push(Subfield {
            code: b'9',
            value: self.number.to_string().into_bytes(),
        });
        subfields.sort_by_key(|field| field.code);
        Field::Data {
            tag: ITEM_TAG,
            indicators: *b"  ",
            subfields,
        }
    }
}

fn append_items(record: &mut Record, items: Vec<Field>) {
    let position = record
        .fields
        .iter()
        .rposition(|field| field.tag() == ITEM_TAG)
        .map_or_else(
            || {
                record
                    .fields
                    .iter()
                    .position(|field| field.tag().as_bytes() > ITEM_TAG.as_bytes())
                    .unwrap_or(record.fields.len())
            },
            |last| last + 1,
        );
    record.fields.splice(position..position, items);
}

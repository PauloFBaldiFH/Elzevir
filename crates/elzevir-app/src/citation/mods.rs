// SPDX-License-Identifier: GPL-3.0-or-later
//! MODS 3.1 (docs/specs/catalogo/citacao-e-intercambio.md, items 10 and 11).
//!
//! Koha: `C4::Record::marc2modsxml` with `MARC21slim2MODS3-1.xsl`, for the parts item 10
//! names.

use elzevir_marc::Record;

use super::{DataField, chop, chopped, control, fields, fields_of, first_field};
use crate::oai::escape;

/// Notes written as plain `note`, every subfield joined.
const OTHER_NOTES: [&[u8; 3]; 40] = [
    b"501", b"502", b"504", b"506", b"507", b"508", b"513", b"514", b"515", b"516", b"522", b"524",
    b"525", b"526", b"530", b"533", b"534", b"535", b"536", b"538", b"540", b"541", b"544", b"545",
    b"546", b"547", b"550", b"552", b"555", b"556", b"561", b"562", b"565", b"567", b"580", b"581",
    b"583", b"584", b"585", b"586",
];

/// Subjects, in the record's order.
const SUBJECTS: [&[u8; 3]; 7] = [b"600", b"610", b"611", b"630", b"650", b"651", b"653"];

/// Series, each kind in turn.
const SERIES: [&[u8; 3]; 4] = [b"800", b"810", b"811", b"830"];

pub(super) fn mods(record: &Record, deposit_url: Option<&str>) -> String {
    let mut xml = Xml::default();
    xml.text.push_str(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<mods xmlns=\"http://www.loc.gov/mods/v3\" \
         xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\" version=\"3.1\" \
         xsi:schemaLocation=\"http://www.loc.gov/mods/v3 \
         http://www.loc.gov/standards/mods/v3/mods-3-1.xsd\">\n",
    );
    xml.depth = 1;
    titles(record, &mut xml);
    names(record, &mut xml);
    resource_type(record, &mut xml);
    for field in fields(record, *b"655") {
        let authority = field.value(b'2');
        let attributes: &[(&str, &str)] = if authority.is_empty() {
            &[]
        } else {
            &[("authority", &authority)]
        };
        xml.leaf("genre", attributes, &field.select_with(b"abvxyz", "-"));
    }
    origin(record, &mut xml);
    description(record, &mut xml);
    notes(record, &mut xml);
    subjects(record, &mut xml);
    classifications(record, &mut xml);
    series(record, &mut xml);
    identifiers(record, &mut xml);
    for url in fields(record, *b"856")
        .map(|field| field.value(b'u'))
        .chain(deposit_url.map(str::to_owned))
        .filter(|url| !url.is_empty())
    {
        xml.open("location", &[]);
        xml.leaf("url", &[], &url);
        xml.close("location");
    }
    for (tag, kind) in [
        (b"506", "restrictionOnAccess"),
        (b"540", "useAndReproduction"),
    ] {
        let codes: &[u8] = if tag == b"506" { b"abcd35" } else { b"abcde35" };
        for field in fields(record, *tag) {
            xml.leaf("accessCondition", &[("type", kind)], &field.select(codes));
        }
    }
    record_info(record, &mut xml);
    xml.text.push_str("</mods>\n");
    xml.text
}

/// An indented XML document under way.
#[derive(Default)]
struct Xml {
    text: String,
    depth: usize,
}

impl Xml {
    fn start(&mut self, name: &str, attributes: &[(&str, &str)]) {
        for _ in 0..self.depth {
            self.text.push_str("  ");
        }
        self.text.push('<');
        self.text.push_str(name);
        for (attribute, value) in attributes {
            self.text.push(' ');
            self.text.push_str(attribute);
            self.text.push_str("=\"");
            self.text.push_str(&escape(value));
            self.text.push('"');
        }
        self.text.push('>');
    }

    fn open(&mut self, name: &str, attributes: &[(&str, &str)]) {
        self.start(name, attributes);
        self.text.push('\n');
        self.depth += 1;
    }

    fn close(&mut self, name: &str) {
        self.depth = self.depth.saturating_sub(1);
        for _ in 0..self.depth {
            self.text.push_str("  ");
        }
        self.text.push_str("</");
        self.text.push_str(name);
        self.text.push_str(">\n");
    }

    /// `<name>value</name>`, unless the value is empty.
    fn leaf(&mut self, name: &str, attributes: &[(&str, &str)], value: &str) {
        if value.trim().is_empty() {
            return;
        }
        self.start(name, attributes);
        self.text.push_str(&escape(value));
        self.text.push_str("</");
        self.text.push_str(name);
        self.text.push_str(">\n");
    }
}

fn titles(record: &Record, xml: &mut Xml) {
    for field in fields(record, *b"245") {
        main_title(field, xml);
    }
    for field in fields(record, *b"246") {
        let label = field.value(b'i');
        let attributes: Vec<(&str, &str)> = if label.is_empty() {
            vec![("type", "alternative")]
        } else {
            vec![("type", "alternative"), ("displayLabel", &label)]
        };
        xml.open("titleInfo", &attributes);
        xml.leaf("title", &[], &chopped(&field.select(b"af")));
        part(field, xml);
        xml.close("titleInfo");
    }
    for (_, field) in fields_of(record, &[b"130", b"240", b"730"]) {
        if field.indicators[1] == b'2' {
            continue;
        }
        let parted = field.has(b'n') || field.has(b'p');
        let title = field
            .subfields
            .iter()
            .enumerate()
            .filter(|(position, subfield)| {
                b"adfklmor".contains(&subfield.code)
                    && (!parted
                        || field
                            .subfields
                            .iter()
                            .skip(position + 1)
                            .any(|later| matches!(later.code, b'n' | b'p')))
            })
            .map(|(_, subfield)| super::text(&subfield.value))
            .collect::<Vec<_>>()
            .join(" ");
        xml.open("titleInfo", &[("type", "uniform")]);
        xml.leaf("title", &[], &chopped(&title));
        part(field, xml);
        xml.close("titleInfo");
    }
    for field in fields(record, *b"740").filter(|field| field.indicators[1] != b'2') {
        xml.open("titleInfo", &[("type", "alternative")]);
        xml.leaf("title", &[], &chopped(&field.select(b"ah")));
        part(field, xml);
        xml.close("titleInfo");
    }
}

/// The `245`: `nonSort` by the second indicator, `title`, `subTitle` and the part.
fn main_title(field: DataField<'_>, xml: &mut Xml) {
    let title = if field.has(b'b') {
        field.special(b'b', b"", b"afgk", b"")
    } else {
        field.select(b"abfgk")
    };
    let title = chopped(&title);
    xml.open("titleInfo", &[]);
    let skip = field.indicators[1]
        .checked_sub(b'0')
        .filter(|skip| (1..=9).contains(skip))
        .map_or(0, usize::from);
    if skip > 0 {
        xml.leaf(
            "nonSort",
            &[],
            &title.chars().take(skip).collect::<String>(),
        );
        xml.leaf("title", &[], &title.chars().skip(skip).collect::<String>());
    } else {
        xml.leaf("title", &[], &title);
    }
    if field.has(b'b') {
        xml.leaf(
            "subTitle",
            &[],
            &chopped(&field.special(b'b', b"b", b"", b"afgk")),
        );
    }
    part(field, xml);
    xml.close("titleInfo");
}

/// Koha: the `part` template: `partNumber` and `partName`.
fn part(field: DataField<'_>, xml: &mut Xml) {
    xml.leaf(
        "partNumber",
        &[],
        &chopped(&field.special(b'n', b"n", b"", b"fgkdlmor")),
    );
    xml.leaf(
        "partName",
        &[],
        &chopped(&field.special(b'p', b"p", b"", b"fgkdlmor")),
    );
}

fn names(record: &Record, xml: &mut Xml) {
    for (tag, kind, creator) in [
        (b"100", "personal", true),
        (b"110", "corporate", true),
        (b"111", "conference", true),
        (b"700", "personal", false),
        (b"710", "corporate", false),
        (b"711", "conference", false),
    ] {
        for field in fields(record, *tag).filter(|field| creator || !field.has(b't')) {
            xml.open("name", &[("type", kind)]);
            match kind {
                "personal" => {
                    personal_name(field, xml);
                    for affiliation in field.all(b'u') {
                        xml.leaf("affiliation", &[], &affiliation);
                    }
                }
                "corporate" => corporate_name(field, xml),
                _ => xml.leaf("namePart", &[], &field.select(b"acdeq")),
            }
            if creator {
                xml.open("role", &[]);
                xml.leaf(
                    "roleTerm",
                    &[("authority", "marcrelator"), ("type", "text")],
                    "creator",
                );
                xml.close("role");
            }
            roles(field, xml);
            xml.close("name");
        }
    }
}

/// Koha: `nameABCDQ`, with `termsOfAddress` and `nameDate`.
fn personal_name(field: DataField<'_>, xml: &mut Xml) {
    xml.leaf("namePart", &[], &chop(&field.select(b"aq"), ":,;/ "));
    xml.leaf(
        "namePart",
        &[("type", "termsOfAddress")],
        &chopped(&field.select(b"bc")),
    );
    for date in field.all(b'd') {
        xml.leaf("namePart", &[("type", "date")], &chopped(&date));
    }
}

/// Koha: `nameABCDN`.
fn corporate_name(field: DataField<'_>, xml: &mut Xml) {
    for name in field.all(b'a') {
        xml.leaf("namePart", &[], &chopped(&name));
    }
    for unit in field.all(b'b') {
        xml.leaf("namePart", &[], &unit);
    }
    xml.leaf("namePart", &[], &field.select(b"cdn"));
}

/// Koha: the `role` template, `$e` as text and `$4` as a code.
fn roles(field: DataField<'_>, xml: &mut Xml) {
    for (code, attributes) in [
        (b'e', &[("type", "text")][..]),
        (b'4', &[("authority", "marcrelator"), ("type", "code")][..]),
    ] {
        for role in field.all(code).filter(|role| !role.trim().is_empty()) {
            xml.open("role", &[]);
            xml.leaf("roleTerm", attributes, &role);
            xml.close("role");
        }
    }
}

fn resource_type(record: &Record, xml: &mut Xml) {
    let leader = &record.leader;
    let type_of_record = leader.get(6).copied().unwrap_or(b' ');
    let kind = match type_of_record {
        b'a' | b't' => "text",
        b'e' | b'f' => "cartographic",
        b'c' | b'd' => "notated music",
        b'i' => "sound recording-nonmusical",
        b'j' => "sound recording-musical",
        b'k' => "still image",
        b'g' => "moving image",
        b'r' => "three dimensional object",
        b'm' => "software, multimedia",
        b'p' => "mixed material",
        _ => "",
    };
    let mut attributes = Vec::new();
    if leader.get(7) == Some(&b'c') {
        attributes.push(("collection", "yes"));
    }
    if matches!(type_of_record, b'd' | b'f' | b'p' | b't') {
        attributes.push(("manuscript", "yes"));
    }
    xml.leaf("typeOfResource", &attributes, kind);
}

/// `originInfo`: the country, the publication, the dates of the `008`, the edition and
/// the issuance.
fn origin(record: &Record, xml: &mut Xml) {
    let fixed = control(record, *b"008").unwrap_or_default();
    let fixed: Vec<char> = fixed.chars().collect();
    let slice = |from: usize, to: usize| -> String {
        fixed
            .get(from..to)
            .map(|chars| chars.iter().collect::<String>())
            .unwrap_or_default()
            .trim()
            .to_owned()
    };
    xml.open("originInfo", &[]);
    let country = slice(15, 18);
    if !country.replace('|', "").is_empty() {
        xml.open("place", &[]);
        xml.leaf(
            "placeTerm",
            &[("type", "code"), ("authority", "marccountry")],
            &country,
        );
        xml.close("place");
    }
    let publication = first_field(record, *b"260")
        .or_else(|| fields(record, *b"264").find(|field| field.indicators[1] == b'1'));
    let mut stated_date = String::new();
    if let Some(field) = publication {
        for place in field.all(b'a') {
            let place = chopped(&place);
            xml.open("place", &[]);
            xml.leaf(
                "placeTerm",
                &[("type", "text")],
                place.trim_start_matches(|c| ".:,;/[ ".contains(c)),
            );
            xml.close("place");
        }
        for subfield in field.subfields {
            let value = super::text(&subfield.value);
            match subfield.code {
                b'b' => xml.leaf("publisher", &[], &chop(&value, ":,;/ ")),
                b'c' => {
                    if stated_date.is_empty() {
                        stated_date = chopped(&value);
                    }
                    xml.leaf("dateIssued", &[], &chopped(&value));
                }
                b'g' => xml.leaf("dateCreated", &[], &value),
                _ => {}
            }
        }
    }
    fixed_dates(
        slice(6, 7).as_str(),
        &slice(7, 11),
        &slice(11, 15),
        &stated_date,
        xml,
    );
    if let Some(field) = first_field(record, *b"250") {
        xml.leaf("edition", &[], &field.value(b'a'));
    }
    let issuance = match record.leader.get(7) {
        Some(b'a' | b'c' | b'd' | b'm') => "monographic",
        Some(b'b' | b'i' | b's') => "continuing",
        _ => "",
    };
    xml.leaf("issuance", &[], issuance);
    xml.close("originInfo");
}

/// The dates of the `008` by its type of date (positions 06 to 14).
fn fixed_dates(kind: &str, first: &str, second: &str, stated: &str, xml: &mut Xml) {
    let marc = ("encoding", "marc");
    match kind {
        "e" | "p" | "r" | "s" | "t" if first != stated => {
            xml.leaf("dateIssued", &[marc], first);
        }
        "c" | "d" | "i" | "k" | "m" | "u" => {
            xml.leaf("dateIssued", &[marc, ("point", "start")], first);
            xml.leaf("dateIssued", &[marc, ("point", "end")], second);
        }
        "q" => {
            let questionable = ("qualifier", "questionable");
            xml.leaf(
                "dateIssued",
                &[marc, ("point", "start"), questionable],
                first,
            );
            xml.leaf(
                "dateIssued",
                &[marc, ("point", "end"), questionable],
                second,
            );
        }
        _ => {}
    }
    if kind == "t" {
        xml.leaf("copyrightDate", &[marc], second);
    }
}

/// `language`, `physicalDescription`, `abstract`, `tableOfContents` and
/// `targetAudience`.
fn description(record: &Record, xml: &mut Xml) {
    let fixed = control(record, *b"008").unwrap_or_default();
    let language: String = fixed.chars().skip(35).take(3).collect();
    let mut languages: Vec<String> = Vec::new();
    if !language.replace(['|', '#', ' '], "").is_empty() {
        languages.push(language);
    }
    for codes in fields(record, *b"041").flat_map(|field| field.all(b'a').collect::<Vec<_>>()) {
        let codes: Vec<char> = codes.chars().collect();
        for code in codes.chunks(3).map(|code| code.iter().collect::<String>()) {
            if code.chars().count() == 3 && !languages.contains(&code) {
                languages.push(code);
            }
        }
    }
    for code in languages {
        xml.open("language", &[]);
        xml.leaf(
            "languageTerm",
            &[("authority", "iso639-2b"), ("type", "code")],
            &code,
        );
        xml.close("language");
    }
    let extents: Vec<String> = fields(record, *b"300")
        .map(|field| field.select(b"abce"))
        .filter(|extent| !extent.trim().is_empty())
        .collect();
    if !extents.is_empty() {
        xml.open("physicalDescription", &[]);
        for extent in extents {
            xml.leaf("extent", &[], &extent);
        }
        xml.close("physicalDescription");
    }
    for (tag, element, codes) in [
        (b"520", "abstract", &b"ab"[..]),
        (b"505", "tableOfContents", &b"agrt"[..]),
        (b"521", "targetAudience", &b"ab"[..]),
    ] {
        for field in fields(record, *tag) {
            xml.leaf(element, &[], &field.select(codes));
        }
    }
}

fn notes(record: &Record, xml: &mut Xml) {
    for field in fields(record, *b"245") {
        for responsibility in field.all(b'c') {
            xml.leaf(
                "note",
                &[("type", "statement of responsibility")],
                &responsibility,
            );
        }
    }
    for (tag, kind) in [(b"500", ""), (b"511", "performers"), (b"518", "venue")] {
        for field in fields(record, *tag) {
            let attributes: &[(&str, &str)] = if kind.is_empty() {
                &[]
            } else {
                &[("type", kind)]
            };
            xml.leaf("note", attributes, &field.value(b'a'));
        }
    }
    for (_, field) in fields_of(record, &OTHER_NOTES) {
        let all: Vec<String> = field
            .subfields
            .iter()
            .map(|subfield| super::text(&subfield.value))
            .collect();
        xml.leaf("note", &[], &all.join(" "));
    }
}

fn subjects(record: &Record, xml: &mut Xml) {
    for (tag, field) in fields_of(record, &SUBJECTS) {
        let tag = tag.as_bytes();
        let authority = subject_authority(field);
        let attributes: Vec<(&str, &str)> = authority
            .as_deref()
            .map(|authority| vec![("authority", authority)])
            .unwrap_or_default();
        xml.open("subject", &attributes);
        match tag {
            b"600" => {
                xml.open("name", &[("type", "personal")]);
                xml.leaf(
                    "namePart",
                    &[("type", "termsOfAddress")],
                    &chopped(&field.select(b"bc")),
                );
                xml.leaf("namePart", &[], &chopped(&field.select(b"aq")));
                for date in field.all(b'd') {
                    xml.leaf("namePart", &[("type", "date")], &chopped(&date));
                }
                roles(field, xml);
                xml.close("name");
            }
            b"610" => {
                xml.open("name", &[("type", "corporate")]);
                for name in field.all(b'a').chain(field.all(b'b')) {
                    xml.leaf("namePart", &[], &name);
                }
                xml.leaf("namePart", &[], &field.select(b"cdnp"));
                roles(field, xml);
                xml.close("name");
            }
            b"611" => {
                xml.open("name", &[("type", "conference")]);
                xml.leaf("namePart", &[], &field.select(b"abcdeqnp"));
                xml.close("name");
            }
            b"630" => {
                xml.open("titleInfo", &[]);
                xml.leaf("title", &[], &chopped(&field.select(b"adfhklor")));
                part(field, xml);
                xml.close("titleInfo");
            }
            b"650" => xml.leaf("topic", &[], &chopped(&field.select(b"abcd"))),
            b"651" => {
                for place in field.all(b'a') {
                    xml.leaf("geographic", &[], &chopped(&place));
                }
            }
            _ => {
                for term in field.all(b'a') {
                    xml.leaf("topic", &[], &term);
                }
            }
        }
        if tag != b"653" {
            for subfield in field.subfields {
                let element = match subfield.code {
                    b'v' | b'x' => "topic",
                    b'y' => "temporal",
                    b'z' => "geographic",
                    _ => continue,
                };
                xml.leaf(element, &[], &chopped(&super::text(&subfield.value)));
            }
        }
        xml.close("subject");
    }
}

/// Koha: `subjectAuthority`, by the second indicator.
fn subject_authority(field: DataField<'_>) -> Option<String> {
    let authority = match field.indicators[1] {
        b'0' => "lcsh",
        b'1' => "lcshac",
        b'2' => "mesh",
        b'3' => "nal",
        b'5' => "csh",
        b'6' => "rvm",
        b'7' => return field.first(b'2'),
        _ => return None,
    };
    Some(authority.to_owned())
}

fn classifications(record: &Record, xml: &mut Xml) {
    let lcc = [("authority", "lcc")];
    for field in fields(record, *b"050") {
        let mut class = String::new();
        for subfield in field.subfields {
            let value = super::text(&subfield.value);
            match subfield.code {
                b'a' => class = value,
                b'b' => xml.leaf("classification", &lcc, &format!("{class} {value}")),
                _ => {}
            }
        }
        // An `$a` with no `$b` after it is a class of its own.
        let after_last_b = field
            .subfields
            .iter()
            .rev()
            .take_while(|subfield| subfield.code != b'b')
            .filter(|subfield| subfield.code == b'a')
            .map(|subfield| super::text(&subfield.value))
            .collect::<Vec<_>>();
        for class in after_last_b.into_iter().rev() {
            xml.leaf("classification", &lcc, &class);
        }
    }
    for field in fields(record, *b"082") {
        let edition = field.value(b'2');
        let mut attributes = vec![("authority", "ddc")];
        if !edition.is_empty() {
            attributes.push(("edition", &edition));
        }
        xml.leaf("classification", &attributes, &field.select(b"ab"));
    }
    for (tag, authority, codes) in [(b"080", "udc", &b"abx"[..]), (b"060", "nlm", &b"ab"[..])] {
        for field in fields(record, *tag) {
            xml.leaf(
                "classification",
                &[("authority", authority)],
                &field.select(codes),
            );
        }
    }
    for field in fields(record, *b"084") {
        let source = field.value(b'2');
        xml.leaf(
            "classification",
            &[("authority", &source)],
            &field.select(b"ab"),
        );
    }
}

fn series(record: &Record, xml: &mut Xml) {
    let statements = fields(record, *b"440")
        .chain(fields(record, *b"490").filter(|field| field.indicators[0] == b'0'));
    for field in statements {
        series_item(&chopped(&field.select(b"av")), field, xml);
    }
    for tag in SERIES {
        for field in fields(record, *tag) {
            let title = match tag {
                b"830" => field.select(b"adfgklmorsv"),
                _ => field.special(b't', b"tfklmorsv", b"", b"g"),
            };
            series_item(&chopped(&title), field, xml);
        }
    }
}

fn series_item(title: &str, field: DataField<'_>, xml: &mut Xml) {
    xml.open("relatedItem", &[("type", "series")]);
    xml.open("titleInfo", &[]);
    xml.leaf("title", &[], title);
    part(field, xml);
    xml.close("titleInfo");
    xml.close("relatedItem");
}

fn identifiers(record: &Record, xml: &mut Xml) {
    for field in fields(record, *b"020") {
        xml.leaf("identifier", &[("type", "isbn")], &field.value(b'a'));
    }
    for (indicator, kind) in [(b'0', "isrc"), (b'2', "ismn")] {
        for field in fields(record, *b"024").filter(|field| field.indicators[0] == indicator) {
            xml.leaf("identifier", &[("type", kind)], &field.value(b'a'));
        }
    }
    for field in fields(record, *b"022") {
        xml.leaf("identifier", &[("type", "issn")], &field.value(b'a'));
    }
    for field in fields(record, *b"010") {
        xml.leaf("identifier", &[("type", "lccn")], field.value(b'a').trim());
    }
    for field in fields(record, *b"856") {
        let url = field.value(b'u');
        let kind = if url.starts_with("urn:doi") || url.starts_with("doi") {
            "doi"
        } else {
            "uri"
        };
        xml.leaf("identifier", &[("type", kind)], &url);
    }
    for field in fields(record, *b"024").filter(|field| field.indicators[0] == b'1') {
        xml.leaf("identifier", &[("type", "upc")], &field.value(b'a'));
    }
}

fn record_info(record: &Record, xml: &mut Xml) {
    xml.open("recordInfo", &[]);
    for field in fields(record, *b"040") {
        xml.leaf(
            "recordContentSource",
            &[("authority", "marcorg")],
            &field.value(b'a'),
        );
    }
    if let Some(fixed) = control(record, *b"008") {
        xml.leaf(
            "recordCreationDate",
            &[("encoding", "marc")],
            &fixed.chars().take(6).collect::<String>(),
        );
    }
    if let Some(changed) = control(record, *b"005") {
        xml.leaf("recordChangeDate", &[("encoding", "iso8601")], &changed);
    }
    if let Some(number) = control(record, *b"001") {
        let source = control(record, *b"003").unwrap_or_default();
        let attributes: &[(&str, &str)] = if source.is_empty() {
            &[]
        } else {
            &[("source", &source)]
        };
        xml.leaf("recordIdentifier", attributes, &number);
    }
    for language in fields(record, *b"040").flat_map(|field| field.all(b'b').collect::<Vec<_>>()) {
        xml.open("languageOfCataloging", &[]);
        xml.leaf(
            "languageTerm",
            &[("authority", "iso639-2b"), ("type", "code")],
            &language,
        );
        xml.close("languageOfCataloging");
    }
    xml.close("recordInfo");
}

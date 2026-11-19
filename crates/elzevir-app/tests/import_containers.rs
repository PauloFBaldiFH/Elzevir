// SPDX-License-Identifier: GPL-3.0-or-later
//! The bounded container reader validates data before handing it to import.
#![expect(
    clippy::expect_used,
    reason = "synthetic package construction must succeed"
)]

use std::io::{Cursor, Write};

use elzevir_app::import::containers::{
    Error, Kind, MAX_ARCHIVE_MEMBERS, MAX_WORKBOOK_CELLS, Selected, Selection, inspect, select,
};
use elzevir_app::import_detection::ImportFormat;
use zip::write::SimpleFileOptions;

fn tiny_zip(name: &str, data: &[u8], method: zip::CompressionMethod) -> Vec<u8> {
    entries(&[(name, data)], method)
}

fn entries(data: &[(&str, &[u8])], method: zip::CompressionMethod) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in data {
        writer
            .start_file(
                *name,
                SimpleFileOptions::default().compression_method(method),
            )
            .expect("synthetic archive header");
        writer.write_all(bytes).expect("synthetic archive content");
    }
    writer.finish().expect("synthetic ZIP").into_inner()
}

fn gzip(name: &str, value: &[u8]) -> Vec<u8> {
    let mut writer = flate2::GzBuilder::new()
        .filename(name)
        .write(Vec::new(), flate2::Compression::default());
    writer.write_all(value).expect("synthetic gzip");
    writer.finish().expect("gzip trailer")
}

fn choice(member: &[&str], sheet: Option<&str>) -> Selection {
    Selection {
        member: member.iter().map(|name| (*name).to_owned()).collect(),
        sheet: sheet.map(str::to_owned),
        format: None,
    }
}

#[test]
fn gzip_members_inside_zip_keep_separate_exact_choices() {
    let one = b"Title,ISBN\nFiction A,9781234567012\n";
    let two = b"Title,ISBN\nFiction B,9781234567029\n";
    let mut gzip_file = gzip("alpha", one);
    gzip_file.extend(gzip("beta", two));
    let archive = tiny_zip("pack.bin", &gzip_file, zip::CompressionMethod::Stored);
    let listing = inspect(&archive).unwrap();
    assert_eq!(listing.entries.len(), 2);
    assert_eq!(listing.entries[1].member, ["pack.bin", "beta"]);
    assert_eq!(listing.entries[1].kind, Kind::File(ImportFormat::Delimited));
    let selected = select(&archive, &choice(&["pack.bin", "beta"], None)).unwrap();
    let Selected::File(data, evidence) = selected else {
        panic!("selected saved CSV");
    };
    assert_eq!(data, two);
    assert_eq!(evidence.format, ImportFormat::Delimited);
    assert!(matches!(
        select(&archive, &choice(&["pack.bin"], None)),
        Err(Error::Selection)
    ));
    let oversized = gzip("expand.txt", &vec![b'x'; 65_536]);
    assert!(matches!(
        inspect(&oversized),
        Err(Error::Limit("compression ratio"))
    ));
}

#[test]
fn refuses_traversal_aliases_special_members_and_a_third_container() {
    for name in [
        "../catalog.csv",
        "/catalog.csv",
        "C:\\catalog.csv",
        "doc/../catalog.csv",
    ] {
        let bytes = tiny_zip(name, b"source", zip::CompressionMethod::Stored);
        assert!(matches!(inspect(&bytes), Err(Error::Name)));
    }
    let aliases = entries(
        &[("A", b"source"), ("a", b"other")],
        zip::CompressionMethod::Stored,
    );
    assert!(matches!(inspect(&aliases), Err(Error::Name)));
    let crossed = entries(
        &[("node", b"data"), ("node/catalog.csv", b"other")],
        zip::CompressionMethod::Stored,
    );
    assert!(matches!(inspect(&crossed), Err(Error::Name)));
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    writer
        .add_symlink("copy", "target", SimpleFileOptions::default())
        .unwrap();
    let link = writer.finish().unwrap().into_inner();
    assert!(matches!(inspect(&link), Err(Error::Member)));
    let first = tiny_zip("catalog.csv", b"source", zip::CompressionMethod::Stored);
    let second = tiny_zip("level-one", &first, zip::CompressionMethod::Stored);
    let third = gzip("level-two", &second);
    assert!(matches!(
        inspect(&third),
        Err(Error::Limit("container depth"))
    ));
}

fn xlsx(sheet: &[u8]) -> Vec<u8> {
    xlsx_parts(sheet, b"<sst/>")
}

fn xlsx_parts(sheet: &[u8], strings: &[u8]) -> Vec<u8> {
    let types = br#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/></Types>"#;
    let book = br#"<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="Livros" sheetId="1" r:id="rId1"/></sheets></workbook>"#;
    let relations = br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/></Relationships>"#;
    let root = br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#;
    entries(
        &[
            ("[Content_Types].xml", types),
            ("_rels/.rels", root),
            ("xl/workbook.xml", book),
            ("xl/_rels/workbook.xml.rels", relations),
            ("xl/sharedStrings.xml", strings),
            ("xl/worksheets/sheet1.xml", sheet),
        ],
        zip::CompressionMethod::Stored,
    )
}

fn ods(table: &str) -> Vec<u8> {
    let document = format!(
        r#"<office:document-content xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:table="urn:oasis:names:tc:opendocument:xmlns:table:1.0" xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0"><office:body><office:spreadsheet><table:table table:name="Livros">{table}</table:table></office:spreadsheet></office:body></office:document-content>"#
    );
    entries(
        &[
            (
                "mimetype",
                b"application/vnd.oasis.opendocument.spreadsheet",
            ),
            ("content.xml", document.as_bytes()),
            (
                "META-INF/manifest.xml",
                br#"<manifest:manifest xmlns:manifest="urn:oasis:names:tc:opendocument:xmlns:manifest:1.0"><manifest:file-entry manifest:full-path="/" manifest:media-type="application/vnd.oasis.opendocument.spreadsheet"/><manifest:file-entry manifest:full-path="content.xml" manifest:media-type="text/xml"/></manifest:manifest>"#,
            ),
        ],
        zip::CompressionMethod::Stored,
    )
}

#[test]
fn recognizes_xlsx_and_ods_by_structure_and_reads_saved_values() {
    let sheet = br#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
<dimension ref="A1:B2"/><sheetData>
<row r="1"><c r="A1" t="inlineStr"><is><t>Title</t></is></c><c r="B1" t="inlineStr"><is><t>Price</t></is></c></row>
<row r="2"><c r="A2" t="inlineStr"><is><t>Fiction</t></is></c><c r="B2"><f>99+99</f><v>4</v></c></row>
</sheetData></worksheet>"#;
    let ods_rows = r#"<table:table-row><table:table-cell office:value-type="string"><text:p>Title</text:p></table:table-cell><table:table-cell office:value-type="string"><text:p>Price</text:p></table:table-cell></table:table-row>
<table:table-row><table:table-cell office:value-type="string"><text:p>Fiction</text:p></table:table-cell><table:table-cell office:value-type="float" office:value="4" table:formula="of:=99+99"/></table:table-row>"#;
    for (pkg, kind) in [(xlsx(sheet), Kind::Xlsx), (ods(ods_rows), Kind::Ods)] {
        let listing = inspect(&pkg).unwrap_or_else(|error| panic!("{kind:?}: {error}"));
        assert_eq!(listing.entries.len(), 1);
        assert_eq!(listing.entries[0].kind, kind);
        assert!(listing.entries[0].member.is_empty());
        assert_eq!(listing.entries[0].sheets, ["Livros"]);
        assert!(matches!(
            select(&pkg, &choice(&[], None)),
            Err(Error::Selection)
        ));
        let Selected::Workbook(rows) = select(&pkg, &choice(&[], Some("Livros"))).unwrap() else {
            panic!("workbook cells");
        };
        assert_eq!(rows[0].cells, ["Title", "Price"]);
        assert_eq!(rows[1].cells, ["Fiction", "4"]);
    }
}

#[test]
fn refuses_coordinate_and_repeat_amplification_and_dtd_before_decoding() {
    let sheet = xlsx(br#"<worksheet><dimension ref="A1:XFD1048576"/><sheetData/></worksheet>"#);
    assert!(matches!(
        inspect(&sheet),
        Err(Error::Limit("worksheet dimensions"))
    ));
    let repeat = ods(
        r#"<table:table-row table:number-rows-repeated="999999"><table:table-cell table:number-columns-repeated="100"/></table:table-row>"#,
    );
    assert!(matches!(
        inspect(&repeat),
        Err(Error::Limit("worksheet dimensions"))
    ));
    let spaces = ods(
        r#"<table:table-row><table:table-cell office:value-type="string"><text:p><text:s text:c="9999999999"/></text:p></table:table-cell></table:table-row>"#,
    );
    assert!(matches!(
        inspect(&spaces),
        Err(Error::Limit("worksheet repetition"))
    ));
    let dtd = xlsx(b"<!DOCTYPE worksheet><worksheet><sheetData/></worksheet>");
    assert!(matches!(inspect(&dtd), Err(Error::Workbook)));
    let strings = format!(r#"<sst uniqueCount="{}"/>"#, MAX_WORKBOOK_CELLS + 1);
    let with_strings = xlsx_parts(b"<worksheet><sheetData/></worksheet>", strings.as_bytes());
    assert!(matches!(
        inspect(&with_strings),
        Err(Error::Limit("workbook shared strings"))
    ));
}

#[test]
fn a_large_declared_expansion_is_refused() {
    let data = vec![b'x'; 65_536];
    let zip = tiny_zip("catalog.txt", &data, zip::CompressionMethod::Deflated);
    assert!(matches!(
        inspect(&zip),
        Err(Error::Limit("compression ratio"))
    ));
}

#[test]
fn the_member_limit_counts_both_archive_levels() {
    let names: Vec<_> = (0..MAX_ARCHIVE_MEMBERS)
        .map(|index| format!("member-{index}"))
        .collect();
    let contents: Vec<_> = names
        .iter()
        .map(|name| (name.as_str(), b"data".as_slice()))
        .collect();
    let full = entries(&contents, zip::CompressionMethod::Stored);
    assert_eq!(inspect(&full).unwrap().entries.len(), MAX_ARCHIVE_MEMBERS);
    let wrapped = tiny_zip("catalog", &full, zip::CompressionMethod::Stored);
    assert!(matches!(
        inspect(&wrapped),
        Err(Error::Limit("member count"))
    ));
}

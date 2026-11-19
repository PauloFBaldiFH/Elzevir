// SPDX-License-Identifier: GPL-3.0-or-later
#![no_main]

use std::io::{Cursor, Write};

use elzevir_app::import::containers::{
    MAX_ARCHIVE_DEPTH, MAX_ARCHIVE_MEMBERS, Selected, Selection, inspect, select,
};
use libfuzzer_sys::fuzz_target;
use zip::write::SimpleFileOptions;

fuzz_target!(|data: &[u8]| {
    check(data);
    let Some((mode, xml)) = data.split_first() else {
        return;
    };
    if xml.len() > 8192 {
        return;
    }
    // Provide valid ZIP metadata and CRCs so mutations reach workbook readers.
    match mode {
        b'O' => check(&workbook(&[
            ("mimetype", b"application/vnd.oasis.opendocument.spreadsheet"),
            ("META-INF/manifest.xml", br#"<manifest:manifest xmlns:manifest="urn:oasis:names:tc:opendocument:xmlns:manifest:1.0"/>"#),
            ("content.xml", xml),
        ])),
        b'X' => check(&workbook(&[
            ("[Content_Types].xml", br#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/></Types>"#),
            ("_rels/.rels", br#"<Relationships><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#),
            ("xl/workbook.xml", br#"<workbook xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="Books" sheetId="1" r:id="rId1"/></sheets></workbook>"#),
            ("xl/_rels/workbook.xml.rels", br#"<Relationships><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/></Relationships>"#),
            ("xl/worksheets/sheet1.xml", xml),
        ])),
        _ => {}
    }
});

fn check(data: &[u8]) {
    let Ok(inventory) = inspect(data) else {
        return;
    };
    assert!(inventory.entries.len() <= MAX_ARCHIVE_MEMBERS);
    for entry in &inventory.entries {
        assert!(entry.member.len() <= MAX_ARCHIVE_DEPTH);
    }
    if let Some(entry) = inventory.entries.first() {
        let choice = Selection {
            member: entry.member.clone(),
            sheet: entry.sheets.first().cloned(),
            format: None,
        };
        if let Ok(Selected::Workbook(rows)) = select(data, &choice) {
            assert!(
                rows.windows(2)
                    .all(|pair| pair[0].number + 1 == pair[1].number)
            );
        }
    }
}

fn workbook(parts: &[(&str, &[u8])]) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, data) in parts {
        zip.start_file(*name, SimpleFileOptions::default())
            .expect("synthetic workbook member");
        zip.write_all(data).expect("synthetic workbook bytes");
    }
    zip.finish()
        .expect("synthetic workbook package")
        .into_inner()
}

// SPDX-License-Identifier: GPL-3.0-or-later
#![no_main]

use std::io::BufReader;

use elzevir_app::import::koha_sql::prepare;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    match prepare(BufReader::with_capacity(17, data)) {
        Ok(records) => {
            let mut previous = None;
            for (record, offset) in records {
                assert!(offset < data.len() as u64);
                assert!(previous.is_none_or(|previous| previous < offset));
                previous = Some(offset);
                let mut iso = Vec::new();
                if record.write_iso2709(&mut iso).is_ok() {
                    let limits = elzevir_marc::Limits::default();
                    assert!(elzevir_marc::RecordRef::parse(&iso, &limits).is_ok());
                }
            }
        }
        Err(error) => assert!(error.offset <= data.len() as u64),
    }
});

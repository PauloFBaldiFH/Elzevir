// SPDX-License-Identifier: GPL-3.0-or-later
#![no_main]

use elzevir_app::import::table::{
    Delimiter, Encoding, MAX_TABLE_CELL_BYTES, MAX_TABLE_COLUMNS, MAX_TABLE_ROW_BYTES, TableReader,
};
use libfuzzer_sys::fuzz_target;
use std::io::BufReader;

fuzz_target!(|data: &[u8]| {
    for delimiter in [Delimiter::Comma, Delimiter::Semicolon, Delimiter::Tab] {
        for encoding in [Encoding::Utf8, Encoding::Latin1] {
            let Ok(mut rows) =
                TableReader::new(BufReader::with_capacity(17, data), delimiter, encoding)
            else {
                continue;
            };
            let mut previous = None;
            let mut row_number = 0;
            for row in rows.by_ref() {
                let Ok(row) = row else {
                    break;
                };
                row_number += 1;
                assert_eq!(row.number, row_number);
                assert!(previous.is_none_or(|offset| offset < row.offset));
                previous = Some(row.offset);
                assert!(row.offset < data.len() as u64);
                assert!(row.cells.len() <= MAX_TABLE_COLUMNS);
                assert!(
                    row.cells
                        .iter()
                        .all(|cell| cell.len() <= MAX_TABLE_CELL_BYTES)
                );
                assert!(row.cells.iter().map(String::len).sum::<usize>() <= MAX_TABLE_ROW_BYTES);
            }
            assert!(rows.next().is_none());
        }
    }
});

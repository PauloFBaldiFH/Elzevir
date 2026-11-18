// SPDX-License-Identifier: GPL-3.0-or-later
//! `MarcEdit` framing, escapes and ISO conversion under bounded input.
#![no_main]

use std::io::BufReader;

use elzevir_marc::{MarcTextReader, MrkLimits};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let limits = MrkLimits {
        max_line_bytes: 8 * 1024,
        max_record_bytes: 16 * 1024,
        max_lines_per_record: 1_000,
        max_input_bytes: 64 * 1024,
        max_records: 64,
    };
    let input = BufReader::with_capacity(256, data);
    for record in MarcTextReader::new(input, limits).flatten() {
        std::hint::black_box(record.write_iso2709(&mut Vec::new()).is_ok());
    }
});

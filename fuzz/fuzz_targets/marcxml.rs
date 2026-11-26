// SPDX-License-Identifier: GPL-3.0-or-later
//! Fuzz target: streaming MARCXML must terminate within its byte/record budgets.
#![no_main]

use elzevir_marc::{MarcXmlReader, XmlLimits};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let limits = XmlLimits {
        max_depth: 8,
        max_element_bytes: 8 * 1024,
        max_record_bytes: 16 * 1024,
        max_elements_per_record: 1_000,
        max_input_bytes: 64 * 1024,
        max_records: 64,
        authorities: true,
    };
    for record in MarcXmlReader::new(data, limits).flatten() {
        // Syntactically valid XML may still exceed ISO field/record limits.
        std::hint::black_box(record.write_iso2709(&mut Vec::new()).is_ok());
    }
});

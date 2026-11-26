// SPDX-License-Identifier: GPL-3.0-or-later
//! Fuzz target: a CQL query from any SRU client must be read or refused with a
//! diagnostic, never crash (docs/specs/opac/sru.md, items 8 to 10).
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(query) = std::str::from_utf8(data) else {
        return;
    };
    std::hint::black_box(elzevir_app::sru::parse_cql(query).is_ok());
});

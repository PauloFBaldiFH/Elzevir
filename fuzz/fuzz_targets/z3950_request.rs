// SPDX-License-Identifier: GPL-3.0-or-later
//! Fuzz target: a request from any Z39.50 client must be read or refused, never crash
//! (docs/specs/opac/z3950.md, items 3 to 17).
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    std::hint::black_box(elzevir_sources::ber::frame_length(data).is_ok());
    std::hint::black_box(elzevir_sources::z3950_server::reads_request(data));
});

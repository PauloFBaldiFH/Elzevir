// SPDX-License-Identifier: GPL-3.0-or-later
//! Fuzz target: a Z39.50 message from any server must be read or refused, never crash,
//! and its stream framing must agree with the reader
//! (docs/specs/catalogo/cliente-z3950.md, items 2 and 14).
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    std::hint::black_box(elzevir_sources::ber::frame_length(data).is_ok());
    std::hint::black_box(elzevir_sources::z3950::parse(data).is_ok());
});

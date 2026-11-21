// SPDX-License-Identifier: GPL-3.0-or-later
//! Fuzz target: an SRU searchRetrieve response from any server must be read or refused,
//! never crash (docs/specs/catalogo/fontes-de-rascunho.md, items 13 and 14).
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    std::hint::black_box(elzevir_sources::parse(data).is_ok());
});

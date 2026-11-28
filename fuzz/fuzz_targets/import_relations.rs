// SPDX-License-Identifier: GPL-3.0-or-later
#![no_main]
use elzevir_app::import::relations::{Selection, analyze};
use libfuzzer_sys::fuzz_target;
use std::io::Cursor;

fuzz_target!(|data: &[u8]| {
    let reader = |_| Ok(Cursor::new(data));
    // The harness models a verified single-member manifest.
    let digests = ["0000000000000000000000000000000000000000000000000000000000000000"];
    let _profile = analyze(&digests, Selection::FolioV1, 1, reader);
    if let Ok(plan) = Selection::parse(data) {
        let _report = analyze(&digests, plan, 1, reader);
    }
    let recipe = br#"{"version":2,"datasets":[{"name":"n","source_sha256":"0000000000000000000000000000000000000000000000000000000000000000","encoding":"json_lines","key":["/id"]}],"relations":[{"child":"n","parent":"n","foreign":["/p"],"cardinality":"one_to_one"}]}"#;
    if let Ok(plan) = Selection::parse(recipe) {
        let _report = analyze(&digests, plan, 1, reader);
    }
});

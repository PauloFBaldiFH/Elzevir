// SPDX-License-Identifier: GPL-3.0-or-later
//! Structural joins tested independently of any source-system profile.
#![expect(clippy::unwrap_used, reason = "fixed test recipes and fixtures")]
use elzevir_app::import::relations::{Selection, analyze};
use serde_json::{Value, json};
use std::io::Cursor;

fn review(files: &[&str], recipe: &Value, from: u64) -> Result<Value, String> {
    let plan = Selection::parse(&serde_json::to_vec(recipe).unwrap())?;
    let report = analyze(files.len(), plan, from, |pos| {
        files
            .get(pos - 1)
            .map(|text| Cursor::new(text.as_bytes()))
            .ok_or_else(|| "absent member".to_owned())
    })?;
    serde_json::to_value(report).map_err(|error| error.to_string())
}

fn dataset(name: &str, member: usize, key: &[&str]) -> Value {
    json!({"name":name,"member":member,"encoding":"json_lines","key":key})
}
fn edge(child: &str, parent: &str, foreign: &[&str], constraint: &str) -> Value {
    json!({"child":child,"parent":parent,"foreign":foreign,"cardinality":constraint})
}

#[test]
fn tuple_components_are_typed_and_found_by_nested_path_with_no_profile() {
    let files = [
        "{\"zone\":\"x|y\",\"n\":\"01\"}\n{\"zone\":\"x\",\"n\":\"y|01\"}\n{\"zone\":\"x\",\"n\":1}\n",
        "{\"key\":\"A\",\"ref\":{\"z\":\"x|y\",\"id\":\"01\"}}\n{\"key\":\"B\",\"ref\":{\"z\":\"x\",\"id\":\"y|01\"}}\n{\"key\":\"C\",\"ref\":{\"z\":\"x\",\"id\":1}}\n{\"key\":\"D\",\"ref\":{\"z\":\"x\",\"id\":\"1\"}}\n",
        "{\"id\":1,\"fk\":\"C\"}\n{\"id\":2,\"fk\":\"D\"}\n",
    ];
    let recipe = json!({"version":1,"datasets":[dataset("shelves",1,&["/zone","/n"]),dataset("bins",2,&["/key"]),dataset("labels",3,&["/id"])],
        "relations":[edge("bins","shelves",&["/ref/z","/ref/id"],"many_to_one"),edge("labels","bins",&["/fk"],"many_to_one")]});
    let r = review(&files, &recipe, 1).unwrap();
    assert_eq!(r["evidence"], "explicit-v1");
    assert_eq!(r["edges"][0]["counts"], json!({"resolved":3,"orphan":1}));
    assert_eq!(
        r["edges"][1]["page"][0]["ancestors"][0]["target"]["line"],
        3
    );
    assert_eq!(
        r["edges"][1]["page"][1]["ancestors"][0]["outcome"],
        "orphan"
    );
    assert_eq!(r["edges"][1]["page"][1]["trace_end"], "unresolved");
    let r = review(&files, &recipe, 4).unwrap();
    assert_eq!(r["edges"][0]["counts"]["resolved"], 3);
    assert_eq!(r["edges"][0]["page"][0]["row"]["line"], 4);
}

#[test]
fn duplicate_claims_bad_components_and_one_to_one_block_all_affected_rows() {
    let files = [
        "{\"k\":\"A\"}\n{\"k\":\"A\"}\n{\"k\":\"B\"}\n{\"k\":0}\n",
        "{\"a\":1,\"b\":\"A\"}\n{\"a\":2,\"b\":\"B\"}\n{\"a\":3,\"b\":\"B\"}\n{\"a\":4,\"b\":null}\n{\"a\":5}\n{\"a\":6,\"b\":\"\"}\n{\"a\":7,\"b\":false}\n{\"a\":8,\"b\":0}\n{\"a\":8,\"b\":\"x\"}\n",
    ];
    let recipe = json!({"version":1,"datasets":[dataset("target",1,&["/k"]),dataset("source",2,&["/a"])],
        "relations":[edge("source","target",&["/b"],"one_to_one")]});
    let r = review(&files, &recipe, 2).unwrap();
    let e = &r["edges"][0];
    assert_eq!(
        e["counts"],
        json!({"ambiguous_parent":1,"cardinality_conflict":2,"null":1,"missing":1,"empty":1,"invalid":1,"duplicate_identity":2})
    );
    assert_eq!(e["examples"]["ambiguous_parent"]["parents"][1]["line"], 2);
    assert_eq!(e["page"][0]["children"][1]["line"], 3);
    assert_eq!(
        r["identities"][0]["counts"],
        json!({"duplicate_identity":2,"resolved":2})
    );
}

#[test]
fn contradictory_envelope_blocks_both_claims_even_when_another_record_agrees() {
    let files = [
        "A\t{\"id\":\"B\"}\taux\nA\t{\"id\":\"A\"}\n",
        "{\"id\":1,\"fk\":\"A\"}\n{\"id\":2,\"fk\":\"B\"}\n",
    ];
    let recipe = json!({"version":1,"datasets":[{"name":"p","member":1,"encoding":"tsv_json","check_envelope":true,"key":["/id"]},dataset("c",2,&["/id"])],"relations":[edge("c","p",&["/fk"],"many_to_one")]});
    let r = review(&files, &recipe, 1).unwrap();
    assert_eq!(r["edges"][0]["counts"], json!({"identity_conflict":2}));
    assert_eq!(r["edges"][0]["page"][0]["candidates"], 2);
    assert_eq!(r["edges"][0]["page"][1]["parents"][0]["offset"], 0);
    assert!(!r.to_string().contains("aux"));
}

#[test]
fn cyclic_self_reference_and_bridge_do_not_invent_a_hierarchy() {
    let files = [
        "{\"id\":1,\"parent\":2}\n{\"id\":2,\"parent\":1}\n",
        "{\"l\":1,\"r\":2}\n",
        "{\"id\":0,\"l\":1,\"r\":2}\n",
    ];
    let recipe = json!({"version":1,"datasets":[dataset("node",1,&["/id"]),dataset("bridge",2,&["/l","/r"]),dataset("marker",3,&["/id"])],"relations":[edge("node","node",&["/parent"],"many_to_one"),edge("bridge","node",&["/l"],"many_to_one"),edge("bridge","node",&["/r"],"many_to_one"),edge("marker","bridge",&["/l","/r"],"one_to_one")]});
    let r = review(&files, &recipe, 1).unwrap();
    assert_eq!(r["edges"][0]["page"][0]["trace_end"], "cycle");
    assert_eq!(r["edges"][3]["page"][0]["trace_end"], "branch");
    assert_eq!(r["edges"][1]["counts"]["resolved"], 1);
    assert_eq!(r["edges"][2]["counts"]["resolved"], 1);
}

#[test]
fn inconsistent_roles_malformed_rows_limits_and_invalid_plans_refuse_without_values() {
    let recipe = json!({"version":1,"datasets":[dataset("x",1,&["/id"])],"relations":[]});
    for bad in [
        "{\"id\":1,\"id\":2}",
        " {\"id\":\"SECRET",
        "{\"nested\":{\"id\":1,\"id\":2}}",
    ] {
        let err = review(&[bad], &recipe, 1).unwrap_err();
        assert!(err.contains("member 1, line 1, byte 0") && !err.contains("SECRET"));
    }
    let too_big = " ".repeat(256 * 1024 + 1);
    assert!(review(&[&too_big], &recipe, 1).is_err());
    assert!(review(&["{}"], &recipe, 0).is_err());
    let wrong = json!({"version":1,"datasets":[dataset("p",1,&["/x","/y"])],"relations":[edge("p","p",&["/fk"],"many_to_one")]});
    assert!(review(&["{}"], &wrong, 1).is_err());
    let mixed=b"x\t{\"title\":\"x\",\"instanceTypeId\":\"t\"}\ny\t{\"instanceId\":\"x\",\"permanentLocationId\":\"L\"}\n";
    assert!(analyze(1, Selection::FolioV1, 1, |_| Ok(Cursor::new(mixed))).is_err());
    assert!(
        analyze(2, Selection::FolioV1, 1, |_| Ok(Cursor::new(
            b"x\t{\"title\":\"x\",\"instanceTypeId\":\"t\"}"
        )))
        .is_err()
    );
}

#[test]
fn absent_role_is_distinct_from_present_empty_file() {
    let recipe = json!({"version":1,"datasets":[dataset("child",1,&["/id"]),{"name":"missing","member":null,"key":["/id"],"encoding":"json_lines"}, dataset("empty",2,&["/id"])],
        "relations":[edge("child","missing",&["/fk"],"many_to_one"),edge("child","empty",&["/fk"],"many_to_one")]});
    let r = review(&["{\"id\":1,\"fk\":7}\n", ""], &recipe, 1).unwrap();
    assert_eq!(r["edges"][0]["counts"], json!({"parent_absent":1}));
    assert_eq!(r["edges"][1]["counts"], json!({"orphan":1}));
}

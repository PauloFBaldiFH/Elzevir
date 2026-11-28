// SPDX-License-Identifier: GPL-3.0-or-later
//! Recognition evidence only. Every proposed binding runs through the generic join.
use super::{Cardinality, Dataset, Encoding, Plan, Relation, RoleEvidence, rows};
use serde_json::Value;
use std::io::BufRead;

const ROLES: [&str; 3] = ["instance", "holding", "item"];

fn signatures(v: &Value, evidence: &mut RoleEvidence) -> [bool; 3] {
    let mut observed = |field: &'static str| {
        let found = if field == "status" {
            v.get(field).is_some_and(Value::is_object)
        } else {
            v.get(field)
                .and_then(Value::as_str)
                .is_some_and(|text| !text.is_empty())
        };
        if found {
            *evidence.fields.entry(field).or_default() += 1;
        }
        found
    };
    let [
        instance_type,
        title,
        location,
        instance,
        call_number,
        material,
        holding,
        status,
    ] = [
        "instanceTypeId",
        "title",
        "permanentLocationId",
        "instanceId",
        "callNumber",
        "materialTypeId",
        "holdingsRecordId",
        "status",
    ]
    .map(&mut observed);
    [
        instance_type && title,
        location && (instance || call_number),
        material && (holding || status),
    ]
}

pub(super) fn recognize<R: BufRead>(
    member_digests: &[&str],
    open: &mut impl FnMut(usize) -> Result<R, String>,
) -> Result<(Plan, Vec<RoleEvidence>), String> {
    let mut observations = Vec::new();
    let mut assignments = [None; 3];
    let mut count = 0;
    for (index, digest) in member_digests.iter().enumerate() {
        let member = index + 1;
        let mut role = None;
        let mut unknown = false;
        let mut evidence = RoleEvidence {
            member,
            ..RoleEvidence::default()
        };
        rows::scan(open(member)?, member, Encoding::TsvJson, |_, _, value| {
            count += 1;
            if count > rows::ROW_LIMIT {
                return Err("relation row budget exceeded".into());
            }
            evidence.rows += 1;
            let signatures = signatures(value, &mut evidence);
            let mut matches = signatures
                .iter()
                .enumerate()
                .filter(|(_, matched)| **matched);
            if let Some((i, _)) = matches.next() {
                if matches.next().is_some()
                    || role.replace(i).is_some_and(|old| old != i)
                    || unknown
                {
                    return Err("ambiguous FOLIO dataset role; review an explicit plan".into());
                }
            } else {
                unknown = true;
                if role.is_some() {
                    return Err("inconsistent FOLIO role evidence".into());
                }
            }
            Ok(())
        })?;
        evidence.role = role.and_then(|i| ROLES.get(i).copied());
        observations.push(evidence);
        if let Some(role) = role
            && let Some((_, first)) =
                super::bound_mut(&mut assignments, role)?.replace((*digest, member))
        {
            return Err(format!(
                "ambiguous {} binding at members {first} and {member}",
                super::bound(&ROLES, role)?
            ));
        }
    }
    if assignments.iter().all(Option::is_none) {
        return Err("no FOLIO role supported by record structure".into());
    }
    let datasets = ROLES
        .iter()
        .zip(assignments)
        .map(|(name, assignment)| Dataset {
            name: (*name).into(),
            source_sha256: assignment.map(|(digest, _)| digest.to_owned()),
            member: None,
            encoding: Encoding::TsvJson,
            key: vec!["/id".into()],
            check_envelope: true,
        })
        .collect();
    let relations = [
        ("holding", "instance", "/instanceId"),
        ("item", "holding", "/holdingsRecordId"),
    ]
    .into_iter()
    .map(|(child, parent, foreign)| Relation {
        child: child.into(),
        parent: parent.into(),
        foreign: vec![foreign.into()],
        cardinality: Cardinality::ManyToOne,
    })
    .collect();
    Ok((
        Plan {
            version: 2,
            datasets,
            relations,
        },
        observations,
    ))
}

// SPDX-License-Identifier: GPL-3.0-or-later
//! Two passes: index identities/unique constraints, then explain each row.
use super::{Cardinality, Dataset, EdgeReport, Finding, Identities, Outcome, Plan, Position, rows};
use serde_json::Value;
use std::collections::{BTreeMap, btree_map::Entry};
use std::io::BufRead;

use super::{bound, bound_mut};

type Key = Vec<u8>;
type Index = BTreeMap<Key, Claims>;

#[derive(Clone, Copy)]
struct Claims {
    first: Position,
    second: Option<Position>,
    count: u32,
    conflict: bool,
}

impl Claims {
    fn examples(self) -> Vec<Position> {
        std::iter::once(self.first).chain(self.second).collect()
    }
}

#[derive(Default)]
struct Budget {
    entries: usize,
    bytes: usize,
    rows: u64,
    checks: usize,
}

impl Budget {
    fn insert(
        &mut self,
        index: &mut Index,
        key: Key,
        pos: Position,
        conflict: bool,
    ) -> Result<(), String> {
        match index.entry(key) {
            Entry::Vacant(entry) => {
                self.entries += 1;
                self.bytes += entry.key().len();
                if self.entries > 400_000 || self.bytes > 48 * 1024 * 1024 {
                    return Err("relation index budget exceeded".into());
                }
                entry.insert(Claims {
                    first: pos,
                    second: None,
                    count: 1,
                    conflict,
                });
            }
            Entry::Occupied(mut entry) => {
                let claims = entry.get_mut();
                claims.count += 1;
                claims.second.get_or_insert(pos);
                claims.conflict |= conflict;
            }
        }
        Ok(())
    }
}

fn component(key: &mut Key, tag: u8, data: &[u8]) -> Result<(), Outcome> {
    if data.is_empty() {
        return Err(Outcome::Empty);
    }
    if key.len() + data.len() + 3 > 4096 {
        return Err(Outcome::Invalid);
    }
    let len = u16::try_from(data.len()).map_err(|_| Outcome::Invalid)?;
    key.push(tag);
    key.extend(len.to_be_bytes());
    key.extend(data);
    Ok(())
}

fn tuple(value: &Value, paths: &[String]) -> Result<Key, Outcome> {
    let mut key = Vec::new();
    for path in paths {
        match value.pointer(path) {
            None => return Err(Outcome::Missing),
            Some(Value::Null) => return Err(Outcome::Null),
            Some(Value::String(text)) => component(&mut key, b's', text.as_bytes())?,
            Some(Value::Number(n)) if n.is_u64() || n.is_i64() => {
                component(&mut key, b'i', n.to_string().as_bytes())?;
            }
            _ => return Err(Outcome::Invalid),
        }
    }
    Ok(key)
}

struct Identity {
    key: Result<Key, Outcome>,
    alternate: Option<Key>,
    conflict: bool,
}

impl Identity {
    fn read(d: &Dataset, envelope: Option<&str>, value: &Value) -> Self {
        let key = tuple(value, &d.key);
        let mut conflict = false;
        let mut alternate = None;
        if d.check_envelope {
            let mut other = Vec::new();
            let check = envelope
                .ok_or(Outcome::Missing)
                .and_then(|s| component(&mut other, b's', s.as_bytes()));
            conflict = check.is_err() || key.as_ref().ok() != Some(&other);
            if conflict && check.is_ok() {
                alternate = Some(other);
            }
        }
        Self {
            key,
            alternate,
            conflict,
        }
    }
    fn outcome(&self, index: &Index) -> Outcome {
        let key = match &self.key {
            Ok(key) => key,
            Err(reason) => return *reason,
        };
        if self.conflict {
            return Outcome::IdentityConflict;
        }
        match index.get(key) {
            Some(hit) if hit.conflict => Outcome::IdentityConflict,
            Some(hit) if hit.count > 1 => Outcome::DuplicateIdentity,
            Some(_) => Outcome::Resolved,
            None => Outcome::Invalid,
        }
    }
}

struct Indices {
    identities: Vec<Index>,
    unique_references: Vec<Index>,
    outgoing: Vec<Vec<usize>>,
}

impl Indices {
    fn build<R: BufRead>(
        plan: &Plan,
        bindings: &[(usize, usize)],
        open: &mut impl FnMut(usize) -> Result<R, String>,
    ) -> Result<Self, String> {
        let mut outgoing = vec![Vec::new(); plan.datasets.len()];
        for (edge, (child, _)) in bindings.iter().enumerate() {
            bound_mut(&mut outgoing, *child)?.push(edge);
        }
        let mut indices = Self {
            identities: vec![Index::new(); plan.datasets.len()],
            unique_references: vec![Index::new(); bindings.len()],
            outgoing,
        };
        let mut budget = Budget::default();
        for (i, d) in plan.datasets.iter().enumerate() {
            let Some(member) = d.member else { continue };
            let outgoing = bound(&indices.outgoing, i)?;
            let index = bound_mut(&mut indices.identities, i)?;
            rows::scan(open(member)?, member, d.encoding, |pos, envelope, value| {
                budget.rows += 1;
                budget.checks += outgoing.len();
                if budget.rows > rows::ROW_LIMIT || budget.checks > 1_000_000 {
                    return Err("relation work budget exceeded".into());
                }
                let id = Identity::read(d, envelope, value);
                if let Ok(key) = id.key {
                    budget.insert(index, key, pos, id.conflict)?;
                }
                if let Some(key) = id.alternate {
                    budget.insert(index, key, pos, true)?;
                }
                for &j in outgoing {
                    let edge = bound(&plan.relations, j)?;
                    if edge.cardinality == Cardinality::OneToOne
                        && let Ok(key) = tuple(value, &edge.foreign)
                    {
                        budget.insert(
                            bound_mut(&mut indices.unique_references, j)?,
                            key,
                            pos,
                            false,
                        )?;
                    }
                }
                Ok(())
            })?;
        }
        Ok(indices)
    }
}

struct Link {
    outcome: Outcome,
    target: Option<Claims>,
    children: Option<Claims>,
}

fn resolve(
    foreign: Result<Key, Outcome>,
    parent_present: bool,
    parent: &Index,
    unique: &Index,
) -> Link {
    let key = match foreign {
        Ok(key) => key,
        Err(outcome) => {
            return Link {
                outcome,
                target: None,
                children: None,
            };
        }
    };
    let target = parent.get(&key).copied();
    let children = unique.get(&key).copied().filter(|hit| hit.count > 1);
    let outcome = match target {
        None if !parent_present => Outcome::ParentAbsent,
        None => Outcome::Orphan,
        Some(hit) if hit.conflict => Outcome::IdentityConflict,
        Some(hit) if hit.count > 1 => Outcome::AmbiguousParent,
        Some(_) if children.is_some() => Outcome::CardinalityConflict,
        Some(_) => Outcome::Resolved,
    };
    Link {
        outcome,
        target,
        children,
    }
}

pub(super) fn evaluate<R: BufRead>(
    plan: &Plan,
    bindings: &[(usize, usize)],
    from: u64,
    mut open: impl FnMut(usize) -> Result<R, String>,
) -> Result<(Vec<Identities>, Vec<EdgeReport>), String> {
    let indices = Indices::build(plan, bindings, &mut open)?;
    let mut lineage = super::lineage::Lineage::new(bindings);
    let mut identities = Vec::new();
    let mut reports: Vec<_> = bindings.iter().map(|_| EdgeReport::default()).collect();
    let mut degrees = vec![BTreeMap::<Position, u32>::new(); bindings.len()];
    for (i, d) in plan.datasets.iter().enumerate() {
        let mut dataset = Identities::default();
        let index = bound(&indices.identities, i)?;
        let outgoing = bound(&indices.outgoing, i)?;
        if let Some(member) = d.member {
            rows::scan(open(member)?, member, d.encoding, |pos, envelope, value| {
                let id = Identity::read(d, envelope, value);
                let status = id.outcome(index);
                *dataset.counts.entry(status).or_default() += 1;
                dataset.examples.entry(status).or_insert(pos);
                for &j in outgoing {
                    let parent = bound(bindings, j)?.1;
                    let link = if status == Outcome::Resolved {
                        resolve(
                            tuple(value, &bound(&plan.relations, j)?.foreign),
                            bound(&plan.datasets, parent)?.member.is_some(),
                            bound(&indices.identities, parent)?,
                            bound(&indices.unique_references, j)?,
                        )
                    } else {
                        Link {
                            outcome: status,
                            target: None,
                            children: id.key.as_ref().ok().and_then(|key| index.get(key)).copied(),
                        }
                    };
                    lineage.record(j, link.outcome, link.target.map(|hit| hit.first))?;
                    let report = bound_mut(&mut reports, j)?;
                    *report.counts.entry(link.outcome).or_default() += 1;
                    if let Some(hit) = link.target.filter(|_| link.outcome == Outcome::Resolved) {
                        let n = bound_mut(&mut degrees, j)?.entry(hit.first).or_default();
                        *n += 1;
                        report.max_children = report.max_children.max(*n);
                    }
                    let visible = (from..from.saturating_add(20)).contains(&pos.line);
                    if visible || !report.examples.contains_key(&link.outcome) {
                        let finding = Finding {
                            ancestors: Vec::new(),
                            trace_end: super::TraceEnd::Unresolved,
                            row: pos,
                            outcome: link.outcome,
                            checked: if status == Outcome::Resolved {
                                match link.outcome {
                                    Outcome::CardinalityConflict => "cardinality",
                                    Outcome::Missing
                                    | Outcome::Null
                                    | Outcome::Empty
                                    | Outcome::Invalid => "foreign_key",
                                    _ => "parent_identity",
                                }
                            } else {
                                "child_identity"
                            },
                            candidates: link.target.map_or(0, |h| h.count),
                            parents: link.target.map(Claims::examples).unwrap_or_default(),
                            children: link.children.map(Claims::examples).unwrap_or_default(),
                        };
                        report
                            .examples
                            .entry(link.outcome)
                            .or_insert_with(|| finding.clone());
                        if visible {
                            report.page.push(finding);
                        }
                    }
                }
                Ok(())
            })?;
        }
        identities.push(dataset);
    }
    for (report, degree) in reports.iter_mut().zip(degrees) {
        report.linked_parents = degree.len();
    }
    lineage.explain(&mut reports, &indices.outgoing, bindings)?;
    Ok((identities, reports))
}

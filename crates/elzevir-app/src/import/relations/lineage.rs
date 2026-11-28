// SPDX-License-Identifier: GPL-3.0-or-later
//! Trace the reviewed page without retaining JSON or expanding a branching graph.
use super::{EdgeReport, Finding, Hop, Outcome, Position, TraceEnd};

type RowSteps = Vec<(Outcome, Option<Position>)>;

pub(super) struct Lineage {
    // Only a rule whose child can itself be reached as a parent needs a lookup.
    rows: Vec<Option<RowSteps>>,
}

impl Lineage {
    pub(super) fn new(bindings: &[(usize, usize)]) -> Self {
        Self {
            rows: bindings
                .iter()
                .map(|(child, _)| {
                    bindings
                        .iter()
                        .any(|(_, parent)| parent == child)
                        .then(Vec::new)
                })
                .collect(),
        }
    }

    pub(super) fn record(
        &mut self,
        rule: usize,
        outcome: Outcome,
        target: Option<Position>,
    ) -> Result<(), String> {
        if let Some(rows) = super::bound_mut(&mut self.rows, rule)? {
            rows.push((outcome, target.filter(|_| outcome == Outcome::Resolved)));
        }
        Ok(())
    }

    pub(super) fn explain(
        &self,
        edges: &mut [EdgeReport],
        outgoing: &[Vec<usize>],
        bindings: &[(usize, usize)],
    ) -> Result<(), String> {
        for (rule, edge) in edges.iter_mut().enumerate() {
            for finding in edge.page.iter_mut().chain(edge.examples.values_mut()) {
                finding.trace_end = self.trace(rule, finding, outgoing, bindings)?;
            }
        }
        Ok(())
    }

    fn trace(
        &self,
        rule: usize,
        finding: &mut Finding,
        outgoing: &[Vec<usize>],
        bindings: &[(usize, usize)],
    ) -> Result<TraceEnd, String> {
        let Some(mut row) = finding
            .parents
            .first()
            .copied()
            .filter(|_| finding.outcome == Outcome::Resolved)
        else {
            return Ok(TraceEnd::Unresolved);
        };
        let mut dataset = super::bound(bindings, rule)?.1;
        let mut visited = vec![(super::bound(bindings, rule)?.0, finding.row)];
        loop {
            if visited.contains(&(dataset, row)) {
                return Ok(TraceEnd::Cycle);
            }
            visited.push((dataset, row));
            let next = match super::bound(outgoing, dataset)?.as_slice() {
                [] => return Ok(TraceEnd::Root),
                [only] => *only,
                _ => return Ok(TraceEnd::Branch),
            };
            if finding.ancestors.len() == 16 {
                return Ok(TraceEnd::DepthLimit);
            }
            let Some(&(outcome, target)) = super::bound(&self.rows, next)?
                .as_ref()
                .and_then(|rows| usize::try_from(row.line - 1).ok().and_then(|n| rows.get(n)))
            else {
                return Ok(TraceEnd::Unresolved);
            };
            finding.ancestors.push(Hop {
                rule: next,
                row,
                outcome,
                target,
            });
            let Some(parent) = target else {
                return Ok(TraceEnd::Unresolved);
            };
            row = parent;
            dataset = super::bound(bindings, next)?.1;
        }
    }
}

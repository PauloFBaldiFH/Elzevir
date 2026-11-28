// SPDX-License-Identifier: GPL-3.0-or-later
//! Versioned, read-only evidence about joins within one private source set.
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::io::BufRead;

mod folio;
mod join;
mod lineage;
mod rows;

/// Record encodings available to a relation plan.
#[derive(Debug, Deserialize, Serialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Encoding {
    /// One complete JSON object per line.
    JsonLines,
    /// External identifier, tab, JSON object and optional auxiliary TSV columns.
    TsvJson,
}

/// An input with an identity expressed as a tuple of JSON pointers.
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Dataset {
    /// Name scoped to this plan.
    pub name: String,
    /// Verified content of this role, or explicitly null when the role is absent.
    #[serde(deserialize_with = "Option::deserialize")]
    pub source_sha256: Option<String>,
    /// Position resolved against the current inventory, never taken from a recipe.
    #[serde(skip)]
    member: Option<usize>,
    /// Source record reader.
    pub encoding: Encoding,
    /// Ordered identity components.
    pub key: Vec<String>,
    /// Require the TSV envelope to independently attest the single identity.
    #[serde(default)]
    pub check_envelope: bool,
}

/// Expected multiplicity of rows referring to a unique parent identity.
#[derive(Debug, Deserialize, Serialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Cardinality {
    /// Multiple children can reference the same parent.
    ManyToOne,
    /// At most one child may claim a parent.
    OneToOne,
}

/// A directed reference to the target dataset's key.
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Relation {
    /// Child dataset name.
    pub child: String,
    /// Parent dataset name.
    pub parent: String,
    /// Child components in target key order.
    pub foreign: Vec<String>,
    /// Constraint checked for all rows, including rows outside the preview page.
    pub cardinality: Cardinality,
}

/// Executable structural evidence; contains field paths, never source values.
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    /// Contract version, currently 2.
    pub version: u8,
    /// Dataset bindings for one source set.
    pub datasets: Vec<Dataset>,
    /// Directed links. Chains and bridge datasets use the same operation.
    pub relations: Vec<Relation>,
}

/// Origin of the proposed dataset roles and keys.
pub enum Selection {
    /// Recognize inventory roles from fields in TSV/JSON records.
    FolioV1,
    /// Review an explicit plan with profile recognition disabled.
    Explicit(Plan),
}

impl Selection {
    /// Reads an explicit relation recipe without including user values in errors.
    ///
    /// # Errors
    /// Rejects invalid or oversized recipes.
    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() > 64 * 1024 {
            return Err("relation plan exceeds 64 KiB".into());
        }
        serde_json::from_slice(bytes)
            .map(Self::Explicit)
            .map_err(|e| {
                format!(
                    "invalid relation plan at line {}, column {}: use v2 with source_sha256 for each dataset",
                    e.line(),
                    e.column()
                )
            })
    }
}

/// Byte position relative to the original member, and one-based record line.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize)]
pub struct Position {
    /// One-based physical line.
    pub line: u64,
    /// Zero-based member byte offset.
    pub offset: u64,
}

/// Exclusive outcome of one identity or directed link.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    /// Exactly one proven identity or target.
    Resolved,
    /// JSON pointer absent.
    Missing,
    /// JSON null.
    Null,
    /// Empty text component.
    Empty,
    /// Unsupported key type.
    Invalid,
    /// Independently supplied identifiers disagree.
    IdentityConflict,
    /// Child identity occurs more than once.
    DuplicateIdentity,
    /// The source for the target role was not supplied.
    ParentAbsent,
    /// Parent source present but has no record for this key.
    Orphan,
    /// Multiple parent records claim this identity.
    AmbiguousParent,
    /// More than one child claims a one-to-one target.
    CardinalityConflict,
}

/// One downstream step of a proven hierarchy, located by zero-based rule index.
#[derive(Clone, Serialize)]
pub struct Hop {
    /// Rule in the reported plan.
    pub rule: usize,
    /// Child row reached by the preceding step.
    pub row: Position,
    /// Decision for that row and rule.
    pub outcome: Outcome,
    /// Next row only when this step resolved uniquely.
    pub target: Option<Position>,
}

/// Why a displayed hierarchy ends here.
#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TraceEnd {
    /// Dataset has no outgoing rule.
    Root,
    /// Consult the separate outgoing rules of this dataset.
    Branch,
    /// At least one step lacks a unique target.
    Unresolved,
    /// A previously visited record was reached again.
    Cycle,
    /// Review further steps through the referenced member and row.
    DepthLimit,
}

/// One row-level explanation. Parent positions belong to the target dataset.
#[derive(Clone, Serialize)]
pub struct Finding {
    /// Child location in the rule's member.
    pub row: Position,
    /// Why the link was resolved or withheld.
    pub outcome: Outcome,
    /// Which dataset's identity or reference produced this decision.
    pub checked: &'static str,
    /// Total target candidates, including those beyond the example limit.
    pub candidates: u32,
    /// First two target locations in source order.
    pub parents: Vec<Position>,
    /// First two child locations implicated by a duplicate identity or cardinality conflict.
    pub children: Vec<Position>,
    /// Continuation through single-parent roles, starting at the resolved parent.
    pub ancestors: Vec<Hop>,
    /// Termination of the displayed path.
    pub trace_end: TraceEnd,
}

/// Complete totals and bounded evidence for one rule, in plan order.
#[derive(Default, Serialize)]
pub struct EdgeReport {
    /// Number of rows per exclusive decision.
    pub counts: BTreeMap<Outcome, u64>,
    /// Number of distinct parents with resolved incoming references.
    pub linked_parents: usize,
    /// Maximum number of resolved children pointing at one parent.
    pub max_children: u32,
    /// First occurrence of each outcome even when outside the requested page.
    pub examples: BTreeMap<Outcome, Finding>,
    /// At most 20 consecutive child rows starting at the requested line.
    pub page: Vec<Finding>,
}

/// Outcomes of checking identities, including datasets with no outgoing links.
#[derive(Default, Serialize)]
pub struct Identities {
    /// One-based member in the current inventory, or absent for an unsupplied role.
    pub member: Option<usize>,
    /// Exclusive outcomes in this source.
    pub counts: BTreeMap<Outcome, u64>,
    /// First row per outcome, located in this dataset.
    pub examples: BTreeMap<Outcome, Position>,
}

/// Structural observations behind a proposed source role.
#[derive(Default, Serialize)]
pub struct RoleEvidence {
    /// Inventory member scanned.
    pub member: usize,
    /// Proposed role when the pattern was consistent across records.
    pub role: Option<&'static str>,
    /// Records inspected in this member.
    pub rows: u64,
    /// Observed type-compatible properties and their row counts, without values.
    pub fields: BTreeMap<&'static str, u64>,
}

/// Read-only review of identities and links.
#[derive(Serialize)]
pub struct Report {
    /// How the plan was obtained.
    pub evidence: &'static str,
    /// Structural evidence when recognition was requested.
    pub observations: Vec<RoleEvidence>,
    /// Effective bindings and key paths.
    pub plan: Plan,
    /// Member positions outside the plan.
    pub unassigned: Vec<usize>,
    /// Identity outcomes per dataset, in plan order.
    pub identities: Vec<Identities>,
    /// Relationship outcomes, in plan order.
    pub edges: Vec<EdgeReport>,
    /// First one-based child row displayed per rule.
    pub from: u64,
}

/// Resolves exact keys in a single immutable batch. The caller supplies verified
/// member digests and a fresh digest-verifying reader for each pass. The index
/// stores only keys, positions and counts.
///
/// # Errors
/// Refuses unknown versions, ambiguous bindings, malformed records and budget overruns.
pub fn analyze<R: BufRead>(
    member_digests: &[&str],
    selection: Selection,
    from: u64,
    mut open: impl FnMut(usize) -> Result<R, String>,
) -> Result<Report, String> {
    let member_count = member_digests.len();
    if !(1..=super::batches::MAX_SOURCE_MEMBERS).contains(&member_count)
        || from == 0
        || !member_digests.iter().all(|s| {
            s.len() == 64
                && s.bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        })
    {
        return Err("invalid relation inventory or page".into());
    }
    let (evidence, (mut plan, observations)) = match selection {
        Selection::FolioV1 => ("folio-v1", folio::recognize(member_digests, &mut open)?),
        Selection::Explicit(plan) => ("explicit-v2", (plan, Vec::new())),
    };
    let bindings = plan.bind(member_digests)?;
    let unassigned = (1..=member_count)
        .filter(|i| !plan.datasets.iter().any(|d| d.member == Some(*i)))
        .collect();
    let (identities, edges) = join::evaluate(&plan, &bindings, from, open)?;
    Ok(Report {
        evidence,
        observations,
        plan,
        unassigned,
        identities,
        edges,
        from,
    })
}

impl Plan {
    fn bind(&mut self, member_digests: &[&str]) -> Result<Vec<(usize, usize)>, String> {
        if self.version != 2
            || self.datasets.is_empty()
            || self.datasets.len() > 64
            || self.relations.len() > 64
        {
            return Err("invalid relation plan version or size".into());
        }
        let mut names = BTreeMap::new();
        let mut positions = BTreeSet::new();
        for (i, dataset) in self.datasets.iter_mut().enumerate() {
            dataset.member = match &dataset.source_sha256 {
                Some(digest) => {
                    let mut matches = member_digests
                        .iter()
                        .enumerate()
                        .filter(|(_, sha)| **sha == digest);
                    let (index, _) = matches.next().ok_or_else(|| {
                        format!(
                            "dataset {}: source digest absent; review the inventory",
                            i + 1
                        )
                    })?;
                    if let Some((second, _)) = matches.next() {
                        return Err(format!(
                            "dataset {}: ambiguous source digest at members {} and {}",
                            i + 1,
                            index + 1,
                            second + 1
                        ));
                    }
                    Some(index + 1)
                }
                None => None,
            };
            if dataset.name.trim().is_empty()
                || dataset.name.len() > 128
                || dataset.name.chars().any(char::is_control)
                || names
                    .insert(&dataset.name, (i, dataset.key.len()))
                    .is_some()
                || dataset.member.is_some_and(|n| !positions.insert(n))
                || !valid_key(&dataset.key)
                || dataset.check_envelope
                    && (dataset.encoding != Encoding::TsvJson || dataset.key.len() != 1)
            {
                return Err("invalid dataset binding or identity".into());
            }
        }
        self.relations
            .iter()
            .map(|edge| {
                let &(child, _) = names.get(&edge.child).ok_or("unknown child dataset")?;
                let &(parent, arity) = names.get(&edge.parent).ok_or("unknown parent dataset")?;
                if !valid_key(&edge.foreign) || edge.foreign.len() != arity {
                    return Err("invalid relation key or arity".into());
                }
                Ok((child, parent))
            })
            .collect()
    }
}

fn bound<T>(values: &[T], index: usize) -> Result<&T, String> {
    values
        .get(index)
        .ok_or_else(|| "invalid relation binding".to_owned())
}

fn bound_mut<T>(values: &mut [T], index: usize) -> Result<&mut T, String> {
    values
        .get_mut(index)
        .ok_or_else(|| "invalid relation binding".to_owned())
}

fn valid_key(paths: &[String]) -> bool {
    (1..=8).contains(&paths.len())
        && paths.iter().collect::<BTreeSet<_>>().len() == paths.len()
        && paths.iter().all(|path| {
            path.starts_with('/')
                && path.len() <= 256
                && !path.chars().any(char::is_control)
                && path
                    .match_indices('~')
                    .all(|(i, _)| matches!(path.as_bytes().get(i + 1), Some(b'0' | b'1')))
        })
}

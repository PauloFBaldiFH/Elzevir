// SPDX-License-Identifier: GPL-3.0-or-later
//! Explicit interpretation of local 949 fields. The normalized and physical sources stay intact.

use std::collections::BTreeSet;

use elzevir_domain::catalog::{BranchId, PolicyId};
use elzevir_marc::{Limits, RecordRef, Tag};
use serde::{Deserialize, Serialize};

use super::RecordFailure;
use crate::catalog::{NativeItem, NewRecord, RecordItems};
use crate::items::ItemFields;

/// Meaning selected by staff for 949 $y.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DateUse {
    /// Retain the value solely in the source.
    SourceOnly,
    /// Transfer an unambiguous ISO calendar date to the exemplar.
    Acquisition,
}

/// Selecting this profile confirms that each 949 $a is a tombo in this installation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    /// Interpretation of the date.
    pub date: DateUse,
    /// Owning library selected from the destination registry.
    pub home_branch: BranchId,
    /// Holding library selected from the destination registry.
    pub holding_branch: BranchId,
    /// Destination circulation policy.
    pub policy: PolicyId,
}

impl Profile {
    pub(super) fn valid(&self) -> bool {
        self.home_branch.0 > 0 && self.holding_branch.0 > 0 && self.policy.0 > 0
    }

    pub(super) fn map(&self, record: &mut NewRecord) -> Result<(), RecordFailure> {
        let refuse = |reason: &str| RecordFailure::Biblivre(reason.into());
        if !self.valid()
            || record
                .instance
                .origin
                .as_deref()
                .is_none_or(|s| s.trim().is_empty())
        {
            return Err(refuse("missing installation or destination registry"));
        }
        if record
            .instance
            .original
            .as_ref()
            .is_none_or(|raw| raw.family != "MARC21")
        {
            return Err(refuse("the 949 profile requires MARC21"));
        }
        let source = RecordRef::parse(
            record.source.as_deref().unwrap_or_default(),
            &Limits::default(),
        )
        .map_err(RecordFailure::Unreadable)?;
        if source.fields_with_tag(Tag::new(*b"952")).next().is_some() {
            return Err(refuse("952 holdings require separate review"));
        }
        let one_control = |tag, required: bool| {
            let mut fields = source.fields_with_tag(Tag::new(tag));
            fields
                .next()
                .map_or(!required, |f| !f.raw().trim_ascii().is_empty())
                && fields.next().is_none()
        };
        if !one_control(*b"001", true) || !one_control(*b"003", false) {
            return Err(refuse(
                "expected one nonempty 001 and at most one nonempty 003",
            ));
        }
        let mut seen = BTreeSet::new();
        let mut items = Vec::new();
        for (index, field) in source.fields_with_tag(Tag::new(*b"949")).enumerate() {
            let error = |reason| RecordFailure::Biblivre(format!("949 #{}: {reason}", index + 1));
            let mut numbers = field.subfield_values(b'a');
            let number = numbers
                .next()
                .and_then(|v| std::str::from_utf8(v).ok())
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .ok_or_else(|| error("missing tombo ($a)"))?;
            if numbers.next().is_some() || !seen.insert(number) {
                return Err(error("repeated tombo ($a)"));
            }
            let acquired_on = if self.date == DateUse::Acquisition {
                let mut dates = field.subfield_values(b'y');
                let date = dates
                    .next()
                    .map(|raw| {
                        let value = std::str::from_utf8(raw)
                            .ok()
                            .map(str::trim)
                            .unwrap_or_default();
                        let parsed = value
                            .parse::<jiff::civil::Date>()
                            .map_err(|_| error("expected YYYY-MM-DD in $y"))?;
                        if parsed.to_string() != value || value.len() != 10 {
                            return Err(error("expected YYYY-MM-DD in $y"));
                        }
                        Ok(value.to_owned())
                    })
                    .transpose()?;
                if dates.next().is_some() {
                    return Err(error("repeated acquisition date ($y)"));
                }
                date
            } else {
                None
            };
            items.push(NativeItem {
                acquired_on,
                fields: ItemFields {
                    inventory_number: Some(number.to_owned()),
                    barcode: None,
                    home_branch: self.home_branch,
                    holding_branch: self.holding_branch,
                    policy: self.policy,
                    call_number: None,
                    location: None,
                    collection: None,
                    copy_number: None,
                    price: None,
                },
                description: elzevir_domain::statement::Description::default(),
            });
        }
        record.items = RecordItems::Biblivre { biblivre: items };
        Ok(())
    }
}

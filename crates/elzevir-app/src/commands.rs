// SPDX-License-Identifier: GPL-3.0-or-later
//! Typed write commands (docs/arquitetura/05-definicoes-de-implementacao.md, §1).
//!
//! Every change to an institution's data is one of these values, sent to the writer
//! thread. Each command is serialized into the audit log, so it must carry only data that
//! may be kept there: never a plain password, only its hash.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::catalog::{CatalogCommand, CatalogOutput, CatalogRefusal};
use crate::circulation_rules::{CirculationCommand, CirculationOutput, CirculationRefusal};
use crate::patrons::{CategoryChange, PatronCommand, PatronOutput, PatronRefusal};
use crate::staff::{Permission, StaffId};

/// Every write the application can make.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum WriteCommand {
    /// Staff account management.
    Staff(StaffCommand),
    /// Catalog: records, items and their vocabularies.
    Catalog(CatalogCommand),
    /// Patrons and their categories.
    Patrons(PatronCommand),
    /// Circulation: rules, and later loans.
    Circulation(CirculationCommand),
}

/// Staff account commands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum StaffCommand {
    /// Creates an account.
    Create {
        /// Unique login.
        login: String,
        /// Argon2id hash (PHC string).
        password_hash: String,
        /// Granted permissions.
        permissions: BTreeSet<Permission>,
    },
}

/// What a successful command returns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WriteOutput {
    /// A staff account was created.
    StaffCreated(StaffId),
    /// A catalog command succeeded.
    Catalog(CatalogOutput),
    /// A patron command succeeded.
    Patrons(PatronOutput),
    /// A circulation command succeeded.
    Circulation(CirculationOutput),
}

/// Why a command was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum WriteRefusal {
    /// The login is already taken.
    #[error("login `{0}` is already in use")]
    LoginTaken(String),
    /// A catalog command was refused.
    #[error(transparent)]
    Catalog(#[from] CatalogRefusal),
    /// A patron command was refused.
    #[error(transparent)]
    Patrons(#[from] PatronRefusal),
    /// A circulation command was refused.
    #[error(transparent)]
    Circulation(#[from] CirculationRefusal),
}

impl WriteCommand {
    /// Short, stable name for the audit log.
    #[must_use]
    pub const fn kind(&self) -> &'static str {
        match self {
            Self::Staff(StaffCommand::Create { .. }) => "staff.create",
            Self::Catalog(CatalogCommand::CreateBranch { .. }) => "catalog.create_branch",
            Self::Catalog(CatalogCommand::CreatePolicy { .. }) => "catalog.create_policy",
            Self::Catalog(CatalogCommand::CreateHub { .. }) => "catalog.create_hub",
            Self::Catalog(CatalogCommand::AddRecord(_)) => "catalog.add_record",
            Self::Catalog(CatalogCommand::AddItem(_)) => "catalog.add_item",
            Self::Catalog(CatalogCommand::ImportRecord(_)) => "catalog.import_record",
            Self::Catalog(CatalogCommand::ReplaceRecord { .. }) => "catalog.replace_record",
            Self::Catalog(CatalogCommand::UpdateWork(_)) => "catalog.update_work",
            Self::Catalog(CatalogCommand::UpdateInstance(_)) => "catalog.update_instance",
            Self::Catalog(CatalogCommand::AddInstance(_)) => "catalog.add_instance",
            Self::Catalog(CatalogCommand::SaveItems(_)) => "catalog.save_items",
            Self::Catalog(CatalogCommand::ChangeItems(_)) => "catalog.change_items",
            Self::Catalog(CatalogCommand::ChangeSavedList(_)) => "catalog.change_saved_list",
            Self::Catalog(CatalogCommand::NameCodes(_)) => "catalog.name_codes",
            Self::Catalog(CatalogCommand::ChangeRegistry(_)) => "catalog.change_registry",
            Self::Catalog(CatalogCommand::SetTypeTranslations(_)) => {
                "catalog.set_type_translations"
            }
            Self::Catalog(CatalogCommand::SetStatements(_)) => "catalog.set_statements",
            Self::Catalog(CatalogCommand::EditStatements(_)) => "catalog.edit_statements",
            Self::Catalog(CatalogCommand::UpdateAgent(_)) => "catalog.update_agent",
            Self::Catalog(CatalogCommand::LinkAuthority(_)) => "catalog.link_authority",
            Self::Catalog(CatalogCommand::UpdateSubject(_)) => "catalog.update_subject",
            Self::Patrons(PatronCommand::ChangeCategory(CategoryChange::Create { .. })) => {
                "patrons.create_category"
            }
            Self::Patrons(PatronCommand::ChangeCategory(CategoryChange::Update { .. })) => {
                "patrons.update_category"
            }
            Self::Patrons(PatronCommand::ChangeCategory(CategoryChange::Delete(_))) => {
                "patrons.delete_category"
            }
            Self::Patrons(PatronCommand::Add(_)) => "patrons.add",
            Self::Patrons(PatronCommand::Update(_)) => "patrons.update",
            Self::Patrons(PatronCommand::View(_)) => "patrons.view",
            Self::Circulation(CirculationCommand::SetRules(_)) => "circ.set_rules",
            Self::Circulation(CirculationCommand::ChangeCalendar(_)) => "circ.change_calendar",
            Self::Circulation(CirculationCommand::Checkout(_)) => "circ.checkout",
            Self::Circulation(CirculationCommand::Checkin(_)) => "circ.checkin",
            Self::Circulation(CirculationCommand::Renew(_)) => "circ.renew",
            Self::Circulation(CirculationCommand::Pay(_)) => "circ.pay",
            Self::Circulation(CirculationCommand::PlaceHold(_)) => "circ.place_hold",
            Self::Circulation(CirculationCommand::CancelHold(_)) => "circ.cancel_hold",
            Self::Circulation(CirculationCommand::SetHoldAside(_)) => "circ.set_hold_aside",
            Self::Circulation(CirculationCommand::ChargeOverdues { .. }) => "circ.charge_overdues",
            Self::Circulation(CirculationCommand::MarkLongOverdue { .. }) => {
                "circ.mark_long_overdue"
            }
            Self::Circulation(CirculationCommand::ExpireHolds { .. }) => "circ.expire_holds",
            Self::Circulation(CirculationCommand::SetOverdueNoticeRule(_)) => {
                "circ.set_overdue_notice_rules"
            }
            Self::Circulation(CirculationCommand::QueueOverdueNotices { .. }) => {
                "circ.queue_overdue_notices"
            }
        }
    }

    /// The permission a staff member needs to submit this command. Checked where a person's
    /// request becomes a command; the writer trusts what it receives.
    #[must_use]
    pub const fn permission(&self) -> Permission {
        match self {
            // Koha: `parameters.manage_circ_rules` for the rules, `tools.edit_calendar` for
            // the calendar; both are the administrators' here. A checkout is the desk's
            // (Koha: `circulate_remaining_permissions`), and so is a payment (Koha:
            // `updatecharges`), and so are holds (Koha: `reserveforothers`).
            Self::Circulation(
                CirculationCommand::Checkout(_)
                | CirculationCommand::Checkin(_)
                | CirculationCommand::Renew(_)
                | CirculationCommand::Pay(_)
                | CirculationCommand::PlaceHold(_)
                | CirculationCommand::CancelHold(_)
                | CirculationCommand::SetHoldAside(_),
            )
            | Self::Patrons(PatronCommand::View(_)) => Permission::Circulation,
            Self::Staff(_) | Self::Circulation(_) => Permission::Administration,
            Self::Catalog(_) => Permission::Cataloguing,
            Self::Patrons(_) => Permission::Patrons,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::WorkUpdate;
    use elzevir_domain::catalog::WorkId;

    #[test]
    fn catalog_edits_need_cataloguing_and_staff_accounts_need_administration() {
        let edit = WriteCommand::Catalog(CatalogCommand::UpdateWork(Box::new(WorkUpdate {
            work: WorkId(1),
            row_version: 1,
            changes: Vec::new(),
        })));
        assert_eq!(edit.permission(), Permission::Cataloguing);
        assert_eq!(edit.kind(), "catalog.update_work");
        let account = WriteCommand::Staff(StaffCommand::Create {
            login: "ana".to_owned(),
            password_hash: String::new(),
            permissions: BTreeSet::new(),
        });
        assert_eq!(account.permission(), Permission::Administration);
    }

    #[test]
    fn permission_names_round_trip() {
        for permission in [
            Permission::StaffInterface,
            Permission::Administration,
            Permission::Cataloguing,
            Permission::Circulation,
            Permission::Patrons,
        ] {
            assert_eq!(Permission::from_name(permission.as_str()), Some(permission));
        }
    }
}

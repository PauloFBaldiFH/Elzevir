// SPDX-License-Identifier: GPL-3.0-or-later
//! What a checkin does to an Item: whether its loan ends, whether it is found, and where
//! it must go next (docs/specs/circulacao/devolucao.md).
//!
//! Fines and suspensions on return come with the patron's account (doc 03, task 3.7) and
//! restrictions; holds found on return come with holds (task 3.8).

use jiff::Zoned;

use crate::catalog::BranchId;

/// Where an Item goes back to after a checkin (the `returnbranch` rule).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReturnPolicy {
    /// To its home branch (`homebranch`, the default).
    Home,
    /// To the branch that held it before the checkin (`holdingbranch`).
    Holding,
    /// It stays where it was returned (`noreturn`).
    Stays,
}

impl ReturnPolicy {
    /// The policy for a stored `returnbranch` value; no rule means home.
    ///
    /// `returnbylibrarygroup` returns home too: Elzevir has no library groups, and Koha
    /// returns home when the checkin branch is not a float sibling.
    #[must_use]
    pub fn from_rule(value: Option<&str>) -> Self {
        match value {
            Some("holdingbranch") => Self::Holding,
            Some("noreturn") => Self::Stays,
            _ => Self::Home,
        }
    }
}

/// Why an Item must go to another branch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferReason {
    /// Back to its home branch (Koha: `ReturnToHome`).
    ReturnToHome,
    /// Back to the branch that held it (Koha: `ReturnToHolding`).
    ReturnToHolding,
}

/// A transfer of the Item that has not arrived yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OpenTransfer {
    /// Where it was sent from.
    pub from: BranchId,
    /// Where it must arrive.
    pub to: BranchId,
}

/// The library's settings for checkins, with Koha's defaults.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CheckinSettings {
    /// `BlockReturnOfWithdrawnItems` (Koha: on).
    pub block_withdrawn: bool,
    /// `BlockReturnOfLostItems` (Koha: off).
    pub block_lost: bool,
    /// `AutomaticItemReturn` (Koha: on): send the Item back at once instead of only saying
    /// it needs to go.
    pub automatic_return: bool,
}

impl Default for CheckinSettings {
    fn default() -> Self {
        Self {
            block_withdrawn: true,
            block_lost: false,
            automatic_return: true,
        }
    }
}

/// What the checkin reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckinFacts {
    /// The branch of the desk.
    pub desk: BranchId,
    /// The Item's home branch.
    pub home: BranchId,
    /// The branch that holds the Item before the checkin.
    pub holding: BranchId,
    /// The Item's `withdrawn` status.
    pub withdrawn: i64,
    /// The Item's `lost` status.
    pub lost: i64,
    /// When the current loan is due, if the Item is on loan.
    pub due: Option<Zoned>,
    /// The Item's transfer that has not arrived, if any.
    pub transfer: Option<OpenTransfer>,
    /// The `returnbranch` rule.
    pub policy: ReturnPolicy,
    /// When.
    pub at: Zoned,
}

/// Where the Item goes after the checkin.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Destination {
    /// It stays at the desk's branch.
    Stays,
    /// It is sent to `to` now (`AutomaticItemReturn`).
    Sent {
        /// Where.
        to: BranchId,
        /// Why.
        reason: TransferReason,
    },
    /// It must go to `to`, and the staff decide when (Koha: `NeedsTransfer`).
    Needs {
        /// Where.
        to: BranchId,
        /// Why.
        reason: TransferReason,
    },
    /// Its open transfer arrived here (Koha: `TransferArrived`).
    Arrived {
        /// Where it came from.
        from: BranchId,
    },
    /// It is in transit to another branch and was returned here (Koha: `WrongTransfer`).
    WrongBranch {
        /// Where it should be.
        to: BranchId,
    },
}

/// What happens to the Item's loan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoanOutcome {
    /// There was no loan (Koha: `NotIssued`).
    NotOnLoan,
    /// The loan ends.
    Ended {
        /// It was due before the checkin.
        overdue: bool,
    },
    /// The Item is lost and `BlockReturnOfLostItems` keeps its loan.
    KeptLost,
}

/// What happens to a lost Item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LostOutcome {
    /// It was not lost.
    NotLost,
    /// It was lost and is found (Koha: `WasLost`).
    Found,
    /// It was lost and stays lost (`BlockReturnOfLostItems`).
    StaysLost,
}

/// What a checkin does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Checkin {
    /// The loan.
    pub loan: LoanOutcome,
    /// The lost status.
    pub lost: LostOutcome,
    /// Where the Item goes next.
    pub destination: Destination,
}

/// Why a checkin did not happen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum CheckinRefusal {
    /// The Item is withdrawn and `BlockReturnOfWithdrawnItems` is on.
    #[error("the item is withdrawn")]
    Withdrawn,
}

/// What a checkin does to the Item described by `facts`.
///
/// # Errors
///
/// [`CheckinRefusal::Withdrawn`] for a withdrawn Item when the settings block its return.
///
/// Koha: `C4::Circulation::AddReturn`, with `AllowReturnToBranch` `anywhere`,
/// `CircControlReturnsBranch` `ItemHomeLibrary` and no branch transfer limits (the
/// defaults); without holds, recalls, fines and suspensions, which come with their tasks.
pub fn checkin(
    facts: &CheckinFacts,
    settings: &CheckinSettings,
) -> Result<Checkin, CheckinRefusal> {
    if facts.withdrawn != 0 && settings.block_withdrawn {
        return Err(CheckinRefusal::Withdrawn);
    }
    let lost = match (facts.lost != 0, settings.block_lost) {
        (false, _) => LostOutcome::NotLost,
        (true, false) => LostOutcome::Found,
        (true, true) => LostOutcome::StaysLost,
    };
    let loan = match &facts.due {
        None => LoanOutcome::NotOnLoan,
        Some(_) if lost == LostOutcome::StaysLost => LoanOutcome::KeptLost,
        Some(due) => LoanOutcome::Ended {
            overdue: *due < facts.at,
        },
    };
    let (return_branch, reason) = match facts.policy {
        ReturnPolicy::Home => (facts.home, Some(TransferReason::ReturnToHome)),
        ReturnPolicy::Holding => (facts.holding, Some(TransferReason::ReturnToHolding)),
        ReturnPolicy::Stays => (facts.desk, None),
    };
    let destination = match (facts.transfer, reason) {
        (Some(transfer), _) if transfer.to == facts.desk => Destination::Arrived {
            from: transfer.from,
        },
        (Some(transfer), _) => Destination::WrongBranch { to: transfer.to },
        (None, Some(reason))
            if return_branch != facts.desk
                && loan != LoanOutcome::KeptLost
                && lost != LostOutcome::StaysLost =>
        {
            if settings.automatic_return {
                Destination::Sent {
                    to: return_branch,
                    reason,
                }
            } else {
                Destination::Needs {
                    to: return_branch,
                    reason,
                }
            }
        }
        (None, _) => Destination::Stays,
    };
    Ok(Checkin {
        loan,
        lost,
        destination,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOME: BranchId = BranchId(1);
    const DESK: BranchId = BranchId(2);
    const OTHER: BranchId = BranchId(3);

    fn at(day: i8) -> Zoned {
        jiff::civil::date(2026, 11, day)
            .at(10, 0, 0, 0)
            .in_tz("America/Sao_Paulo")
            .unwrap()
    }

    fn facts() -> CheckinFacts {
        CheckinFacts {
            desk: DESK,
            home: HOME,
            holding: OTHER,
            withdrawn: 0,
            lost: 0,
            due: Some(at(20)),
            transfer: None,
            policy: ReturnPolicy::Home,
            at: at(19),
        }
    }

    fn run(facts: &CheckinFacts) -> Checkin {
        checkin(facts, &CheckinSettings::default()).unwrap()
    }

    #[test]
    fn a_loan_ends_and_the_item_goes_home() {
        let done = run(&facts());
        assert_eq!(done.loan, LoanOutcome::Ended { overdue: false });
        assert_eq!(
            done.destination,
            Destination::Sent {
                to: HOME,
                reason: TransferReason::ReturnToHome
            }
        );
        let late = run(&CheckinFacts {
            at: at(21),
            ..facts()
        });
        assert_eq!(late.loan, LoanOutcome::Ended { overdue: true });
        let manual = checkin(
            &facts(),
            &CheckinSettings {
                automatic_return: false,
                ..CheckinSettings::default()
            },
        )
        .unwrap();
        assert_eq!(
            manual.destination,
            Destination::Needs {
                to: HOME,
                reason: TransferReason::ReturnToHome
            }
        );
    }

    #[test]
    fn the_return_rule_chooses_where_the_item_goes() {
        let holding = run(&CheckinFacts {
            policy: ReturnPolicy::Holding,
            ..facts()
        });
        assert_eq!(
            holding.destination,
            Destination::Sent {
                to: OTHER,
                reason: TransferReason::ReturnToHolding
            }
        );
        let stays = run(&CheckinFacts {
            policy: ReturnPolicy::Stays,
            ..facts()
        });
        assert_eq!(stays.destination, Destination::Stays);
        let at_home = run(&CheckinFacts {
            desk: HOME,
            ..facts()
        });
        assert_eq!(at_home.destination, Destination::Stays);
        assert_eq!(ReturnPolicy::from_rule(None), ReturnPolicy::Home);
        assert_eq!(
            ReturnPolicy::from_rule(Some("returnbylibrarygroup")),
            ReturnPolicy::Home
        );
    }

    #[test]
    fn an_item_not_on_loan_is_still_checked_in() {
        let done = run(&CheckinFacts {
            due: None,
            ..facts()
        });
        assert_eq!(done.loan, LoanOutcome::NotOnLoan);
        assert!(matches!(done.destination, Destination::Sent { .. }));
    }

    #[test]
    fn open_transfers_arrive_or_stay_on_their_way() {
        let arrived = run(&CheckinFacts {
            due: None,
            transfer: Some(OpenTransfer {
                from: OTHER,
                to: DESK,
            }),
            ..facts()
        });
        assert_eq!(arrived.destination, Destination::Arrived { from: OTHER });
        let wrong = run(&CheckinFacts {
            due: None,
            transfer: Some(OpenTransfer {
                from: OTHER,
                to: HOME,
            }),
            ..facts()
        });
        assert_eq!(wrong.destination, Destination::WrongBranch { to: HOME });
    }

    #[test]
    fn withdrawn_and_lost_items_follow_the_settings() {
        let withdrawn = CheckinFacts {
            withdrawn: 1,
            ..facts()
        };
        assert_eq!(
            checkin(&withdrawn, &CheckinSettings::default()),
            Err(CheckinRefusal::Withdrawn)
        );
        let allowed = CheckinSettings {
            block_withdrawn: false,
            ..CheckinSettings::default()
        };
        assert!(matches!(
            checkin(&withdrawn, &allowed).unwrap().loan,
            LoanOutcome::Ended { .. }
        ));
        let lost = CheckinFacts { lost: 2, ..facts() };
        let found = run(&lost);
        assert_eq!(found.lost, LostOutcome::Found);
        assert!(matches!(found.loan, LoanOutcome::Ended { .. }));
        let blocked = checkin(
            &lost,
            &CheckinSettings {
                block_lost: true,
                ..CheckinSettings::default()
            },
        )
        .unwrap();
        assert_eq!(
            (blocked.loan, blocked.lost),
            (LoanOutcome::KeptLost, LostOutcome::StaysLost)
        );
        assert_eq!(blocked.destination, Destination::Stays);
    }
}

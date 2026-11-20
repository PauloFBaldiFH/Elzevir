// SPDX-License-Identifier: GPL-3.0-or-later
//! Overdue notice rules and the daily queue of notices through the real store
//! (docs/specs/circulacao/avisos.md).
#![expect(
    clippy::unwrap_used,
    clippy::panic,
    reason = "integration test: helper functions fail the test on error"
)]

use elzevir_app::catalog::{
    CatalogCommand, CatalogOutput, CatalogReader, ItemData, ItemStatus, NewInstance, NewItem,
    NewRecord, RecordItems,
};
use elzevir_app::circulation_rules::{
    CirculationCommand, CirculationOutput, CirculationRefusal, OverdueNoticeRule, RuleChange,
};
use elzevir_app::codes::{CodeTable, NamedCode};
use elzevir_app::commands::{WriteCommand, WriteOutput, WriteRefusal};
use elzevir_app::loans::{CheckoutRefusal, CheckoutRequest};
use elzevir_app::notices::{NoticeAbout, NoticeOutcome, NoticeQueue};
use elzevir_app::patrons::{
    CategoryChange, PatronCommand, PatronFields, PatronOutput, ReadingHistory,
};
use elzevir_domain::catalog::{BranchId, InstanceId};
use elzevir_domain::checkout::Impossible;
use elzevir_domain::circulation_rules::RuleScopeKey;
use elzevir_domain::long_overdue::LongOverdueSettings;
use elzevir_domain::notice::{LevelsError, NoticeLevel, OverdueLevel, Transports};
use elzevir_domain::patron::{Enrolment, PatronCategoryId, PatronId};
use elzevir_domain::suspension::{RestrictionKind, SuspensionEnd};
use elzevir_store::{CommandFailed, ElzevirHandler, ElzevirStore, StoreConfig};
use jiff::Zoned;
use jiff::civil::date;

fn submit(
    store: &ElzevirStore,
    command: WriteCommand,
) -> Result<WriteOutput, CommandFailed<WriteRefusal>> {
    store.submit(command).unwrap().wait()
}

fn at(day: &str) -> Zoned {
    format!("{day}T10:00:00-03:00[America/Sao_Paulo]")
        .parse()
        .unwrap()
}

const EMAIL: Transports = Transports {
    email: true,
    print: false,
};

const BOTH: Transports = Transports {
    email: true,
    print: true,
};

fn level(level: NoticeLevel, delay: u16, transports: Transports) -> OverdueLevel {
    OverdueLevel {
        level,
        delay,
        transports,
        restrict: false,
    }
}

struct Library {
    store: ElzevirStore,
    central: BranchId,
    annex: BranchId,
    adults: PatronCategoryId,
    children: PatronCategoryId,
    instance: InstanceId,
}

fn named(store: &ElzevirStore, table: CodeTable, code: &str, name: &str) {
    submit(
        store,
        WriteCommand::Catalog(CatalogCommand::NameCodes(vec![NamedCode {
            table,
            code: code.to_owned(),
            name: name.to_owned(),
        }])),
    )
    .unwrap();
}

fn category(store: &ElzevirStore, name: &str) -> PatronCategoryId {
    let Ok(WriteOutput::Patrons(PatronOutput::CategoryChanged(Some(category)))) = submit(
        store,
        WriteCommand::Patrons(PatronCommand::ChangeCategory(CategoryChange::Create {
            name: name.to_owned(),
            enrolment: Enrolment::Until(date(2027, 12, 31)),
        })),
    ) else {
        panic!("no category");
    };
    category
}

fn library(dir: &tempfile::TempDir) -> Library {
    let store = ElzevirStore::open(
        &dir.path().join("elzevir.db"),
        &StoreConfig::default(),
        ElzevirHandler,
    )
    .unwrap();
    named(&store, CodeTable::Branch, "B1", "Central");
    named(&store, CodeTable::Branch, "B2", "Anexo");
    named(&store, CodeTable::ItemType, "P1", "Livros");
    let codes = store.codes().unwrap();
    let branch = |name: &str| codes.branches.iter().find(|b| b.name == name).unwrap().id;
    let (central, annex) = (branch("Central"), branch("Anexo"));
    let (adults, children) = (category(&store, "Adulto"), category(&store, "Infantil"));
    let Ok(WriteOutput::Catalog(CatalogOutput::Record { instance, .. })) = submit(
        &store,
        WriteCommand::Catalog(CatalogCommand::AddRecord(Box::new(NewRecord {
            work_title: "Dom Casmurro".to_owned(),
            work_description: elzevir_domain::statement::Description::default(),
            hub: None,
            relations: Vec::new(),
            classification: None,
            classification_scheme: None,
            summary: None,
            audience: None,
            languages: Vec::new(),
            contributions: Vec::new(),
            subjects: Vec::new(),
            items: RecordItems::default(),
            source: None,
            instance: NewInstance {
                title: "Dom Casmurro".to_owned(),
                ..NewInstance::default()
            },
        }))),
    ) else {
        panic!("no record");
    };
    submit(
        &store,
        WriteCommand::Circulation(CirculationCommand::SetRules(RuleChange {
            scope: RuleScopeKey::default(),
            values: vec![
                ("maxissueqty".to_owned(), Some("9".to_owned())),
                ("issuelength".to_owned(), Some("14".to_owned())),
            ],
        })),
    )
    .unwrap();
    Library {
        store,
        central,
        annex,
        adults,
        children,
        instance,
    }
}

impl Library {
    fn patron(
        &self,
        cardnumber: &str,
        category: PatronCategoryId,
        email: Option<&str>,
    ) -> PatronId {
        let Ok(WriteOutput::Patrons(PatronOutput::Added(patron))) = submit(
            &self.store,
            WriteCommand::Patrons(PatronCommand::Add(Box::new(PatronFields {
                cardnumber: cardnumber.to_owned(),
                surname: "Silva".to_owned(),
                first_name: None,
                cpf: None,
                date_of_birth: None,
                email: email.map(str::to_owned),
                phone: None,
                address: None,
                branch: self.central,
                category,
                enrolled: date(2026, 1, 31),
                expires: None,
                staff_note: None,
                reading_history: ReadingHistory::NotKept,
            }))),
        ) else {
            panic!("no patron");
        };
        patron
    }

    /// Lends a new Item to the patron at the desk on `day`: due 14 days later.
    fn lend(&self, barcode: &str, patron: PatronId, desk: BranchId, day: &str) {
        submit(
            &self.store,
            WriteCommand::Catalog(CatalogCommand::AddItem(Box::new(NewItem {
                instance: self.instance,
                item: ItemData {
                    barcode: Some(barcode.to_owned()),
                    inventory_number: None,
                    home_branch: "B1".to_owned(),
                    holding_branch: "B1".to_owned(),
                    item_type: "P1".to_owned(),
                    call_number: None,
                    location: None,
                    collection: None,
                    price: None,
                    replacement_price: None,
                    copy_number: None,
                    status: ItemStatus::default(),
                    acquired_on: None,
                },
            }))),
        )
        .unwrap();
        let done = submit(
            &self.store,
            WriteCommand::Circulation(CirculationCommand::Checkout(CheckoutRequest {
                patron,
                barcode: barcode.to_owned(),
                desk,
                on_site: false,
                confirmed: false,
                at: at(day),
            })),
        );
        assert!(
            matches!(
                done,
                Ok(WriteOutput::Circulation(CirculationOutput::CheckedOut(_)))
            ),
            "{done:?}"
        );
    }

    fn rule(
        &self,
        branch: Option<BranchId>,
        category: PatronCategoryId,
        levels: Vec<OverdueLevel>,
    ) -> Result<WriteOutput, CommandFailed<WriteRefusal>> {
        submit(
            &self.store,
            WriteCommand::Circulation(CirculationCommand::SetOverdueNoticeRule(
                OverdueNoticeRule {
                    branch,
                    category,
                    levels,
                },
            )),
        )
    }

    fn run(&self, day: &str) -> usize {
        match submit(
            &self.store,
            WriteCommand::Circulation(CirculationCommand::QueueOverdueNotices { at: at(day) }),
        ) {
            Ok(WriteOutput::Circulation(CirculationOutput::OverdueNoticesQueued { queued })) => {
                queued
            }
            other => panic!("{other:?}"),
        }
    }

    /// Each notice as patron, level, transport, address and the barcodes it lists.
    fn notices(&self) -> Vec<(i64, i64, String, Option<String>, String)> {
        self.store
            .readers()
            .read(|c| {
                let mut statement = c.prepare(
                    "SELECT n.patron_id, n.level, n.transport, n.to_address,
                            group_concat(i.barcode, ' ')
                     FROM notices n JOIN notice_items x ON x.notice_id = n.id
                     JOIN items i ON i.id = x.item_id
                     GROUP BY n.id ORDER BY n.id",
                )?;
                statement
                    .query_map([], |row| {
                        Ok((
                            row.get(0)?,
                            row.get(1)?,
                            row.get(2)?,
                            row.get(3)?,
                            row.get(4)?,
                        ))
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()
            })
            .unwrap()
    }
}

#[test]
fn a_level_goes_out_on_its_exact_day_and_only_once() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(&dir);
    let ana = library.patron("1", library.adults, Some("ana@example.org"));
    // Due 2026-11-16; 3 and 5 days late on the 19th and the 21st.
    library.lend("0001", ana, library.central, "2026-11-02");
    library.lend("0002", ana, library.central, "2026-11-04");
    library
        .rule(
            None,
            library.adults,
            vec![
                level(NoticeLevel::Second, 5, EMAIL),
                level(NoticeLevel::First, 3, EMAIL),
            ],
        )
        .unwrap();

    assert_eq!(library.run("2026-11-18"), 0);
    assert_eq!(library.run("2026-11-19"), 1);
    assert_eq!(library.run("2026-11-19"), 0, "not twice on the same day");
    assert_eq!(library.run("2026-11-20"), 0);
    // On the 21st, 0001 is 5 days late and 0002 is 3: one notice of each level.
    assert_eq!(library.run("2026-11-21"), 2);
    let ana = ana.0;
    let mail = Some("ana@example.org".to_owned());
    assert_eq!(
        library.notices(),
        [
            (ana, 1, "email".to_owned(), mail.clone(), "0001".to_owned()),
            (ana, 1, "email".to_owned(), mail.clone(), "0002".to_owned()),
            (ana, 2, "email".to_owned(), mail, "0001".to_owned()),
        ]
    );
}

#[test]
fn the_rules_of_the_loan_branch_win_and_any_branch_only_when_it_has_none() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(&dir);
    let ana = library.patron("1", library.adults, None);
    let bia = library.patron("2", library.children, None);
    library.lend("0001", ana, library.central, "2026-11-02");
    library.lend("0002", bia, library.central, "2026-11-02");
    library.lend("0003", bia, library.annex, "2026-11-02");
    library
        .rule(
            None,
            library.children,
            vec![level(NoticeLevel::First, 3, BOTH)],
        )
        .unwrap();
    // The central branch has a rule, for adults only: children there get nothing.
    library
        .rule(
            Some(library.central),
            library.adults,
            vec![level(NoticeLevel::First, 3, BOTH)],
        )
        .unwrap();

    assert_eq!(library.run("2026-11-19"), 2);
    // No e-mail: one paper notice each, from the central branch for Ana and from any
    // branch's rule at the annex for Bia.
    assert_eq!(
        library.notices(),
        [
            (ana.0, 1, "print".to_owned(), None, "0001".to_owned()),
            (bia.0, 1, "print".to_owned(), None, "0003".to_owned()),
        ]
    );
}

#[test]
fn a_lost_item_gets_no_notice() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(&dir);
    let ana = library.patron("1", library.adults, Some("ana@example.org"));
    library.lend("0001", ana, library.central, "2026-11-02");
    library
        .rule(
            None,
            library.adults,
            vec![level(NoticeLevel::First, 3, EMAIL)],
        )
        .unwrap();
    submit(
        &library.store,
        WriteCommand::Circulation(CirculationCommand::MarkLongOverdue {
            at: at("2026-11-18"),
            settings: LongOverdueSettings {
                days: 1,
                max_days: 366,
                lost_status: 2,
                charge: false,
                mark_returned: false,
            },
        }),
    )
    .unwrap();
    assert_eq!(library.run("2026-11-19"), 0);
}

#[test]
fn levels_out_of_order_or_an_unknown_scope_are_refused_and_no_level_removes() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(&dir);
    let refused = |result| match result {
        Err(CommandFailed::Rejected(WriteRefusal::Circulation(refusal))) => refusal,
        other => panic!("{other:?}"),
    };
    assert_eq!(
        refused(library.rule(
            None,
            library.adults,
            vec![
                level(NoticeLevel::First, 5, EMAIL),
                level(NoticeLevel::Second, 4, EMAIL),
            ]
        )),
        CirculationRefusal::NoticeLevels(LevelsError::OutOfOrder)
    );
    assert_eq!(
        refused(library.rule(
            Some(BranchId(999)),
            library.adults,
            vec![level(NoticeLevel::First, 5, EMAIL)]
        )),
        CirculationRefusal::UnknownBranch
    );
    library
        .rule(
            None,
            library.adults,
            vec![level(NoticeLevel::First, 5, EMAIL)],
        )
        .unwrap();
    // A category with a notice rule cannot be deleted; with the rule gone, it can.
    let delete = || {
        submit(
            &library.store,
            WriteCommand::Patrons(PatronCommand::ChangeCategory(CategoryChange::Delete(
                library.adults,
            ))),
        )
    };
    assert!(delete().is_err());
    library.rule(None, library.adults, Vec::new()).unwrap();
    assert!(delete().is_ok());
}

#[test]
fn a_restricting_level_blocks_the_patron_until_the_staff_lift_it() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(&dir);
    let ana = library.patron("1", library.adults, None);
    library.lend("0001", ana, library.central, "2026-11-02");
    library
        .rule(
            None,
            library.adults,
            vec![OverdueLevel {
                restrict: true,
                ..level(NoticeLevel::First, 3, Transports::default())
            }],
        )
        .unwrap();
    let lift = || {
        submit(
            &library.store,
            WriteCommand::Circulation(CirculationCommand::LiftRestriction {
                patron: ana,
                kind: RestrictionKind::Overdues,
            }),
        )
    };
    assert!(matches!(
        lift(),
        Err(CommandFailed::Rejected(WriteRefusal::Circulation(
            CirculationRefusal::NoRestriction
        )))
    ));
    // No transport: nothing queued, but the patron is restricted.
    assert_eq!(library.run("2026-11-19"), 0);
    let checkout = || {
        submit(
            &library.store,
            WriteCommand::Circulation(CirculationCommand::Checkout(CheckoutRequest {
                patron: ana,
                barcode: "0001".to_owned(),
                desk: library.central,
                on_site: false,
                confirmed: true,
                at: at("2026-11-19"),
            })),
        )
    };
    match checkout() {
        Err(CommandFailed::Rejected(WriteRefusal::Circulation(CirculationRefusal::Checkout(
            CheckoutRefusal::Impossible(impossible),
        )))) => assert!(impossible.contains(&Impossible::Suspended(SuspensionEnd::WhenLifted))),
        other => panic!("{other:?}"),
    }
    assert!(matches!(
        lift(),
        Ok(WriteOutput::Circulation(
            CirculationOutput::RestrictionLifted
        ))
    ));
    // The run that comes back later the same day does not restrict again.
    assert_eq!(library.run("2026-11-19"), 0);
    let restricted: i64 = library
        .store
        .readers()
        .read(|c| {
            c.query_row("SELECT count(*) FROM patron_restrictions", [], |row| {
                row.get(0)
            })
        })
        .unwrap();
    assert_eq!(restricted, 0);
}

#[test]
fn the_sender_and_the_print_screen_read_pending_notices_with_their_items() {
    let dir = tempfile::tempdir().unwrap();
    let library = library(&dir);
    let ana = library.patron("1", library.adults, Some("ana@example.org"));
    let bia = library.patron("2", library.adults, None);
    library.lend("0001", ana, library.central, "2026-11-02");
    library.lend("0002", ana, library.central, "2026-11-02");
    library.lend("0003", bia, library.central, "2026-11-02");
    library
        .rule(
            None,
            library.adults,
            vec![level(NoticeLevel::First, 3, EMAIL)],
        )
        .unwrap();
    assert_eq!(library.run("2026-11-19"), 2);

    // Bia has no e-mail: her notice is on paper and the sender does not see it.
    let pending = library.store.pending_emails(10).unwrap();
    assert_eq!(pending.len(), 1);
    let notice = &pending[0];
    assert_eq!(
        (
            notice.to.as_deref(),
            notice.name.as_str(),
            notice.library.as_str()
        ),
        (Some("ana@example.org"), "Silva", "Central")
    );
    // The print screen sees Bia's, with no e-mail address.
    let printed = library.store.pending_prints(10).unwrap();
    assert_eq!(printed.len(), 1);
    assert_eq!(printed[0].to, None);
    assert_eq!(printed[0].items[0].barcode, "0003");
    assert_eq!(notice.about, NoticeAbout::Overdue(NoticeLevel::First));
    let barcodes: Vec<_> = notice
        .items
        .iter()
        .map(|item| item.barcode.as_str())
        .collect();
    assert_eq!(barcodes, ["0001", "0002"]);
    assert_eq!(notice.items[0].title, "Dom Casmurro");

    let mark = |outcome| {
        submit(
            &library.store,
            WriteCommand::Circulation(CirculationCommand::MarkNotice {
                notice: notice.id,
                outcome,
                at: at("2026-11-19"),
            }),
        )
        .unwrap()
    };
    mark(NoticeOutcome::Sent);
    assert!(library.store.pending_emails(10).unwrap().is_empty());
    // A notice already sent stays sent.
    mark(NoticeOutcome::Failed);
    let status: String = library
        .store
        .readers()
        .read(|c| {
            c.query_row(
                "SELECT status FROM notices WHERE transport = 'email'",
                [],
                |row| row.get(0),
            )
        })
        .unwrap();
    assert_eq!(status, "sent");
}

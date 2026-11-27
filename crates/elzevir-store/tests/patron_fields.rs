// SPDX-License-Identifier: GPL-3.0-or-later
//! The registry of additional patron fields through the real store
//! (docs/specs/circulacao/patron-attributes.md, items 1 to 7).
#![expect(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "integration test: helper functions fail the test on error"
)]

use elzevir_app::catalog::{CatalogCommand, CatalogReader};
use elzevir_app::codes::{CodeTable, NamedCode};
use elzevir_app::commands::{WriteCommand, WriteOutput, WriteRefusal};
use elzevir_app::patron_fields::{FieldChange, FieldKind, FieldOption, PatronFieldSpec};
use elzevir_app::patrons::{
    CategoryChange, PatronCommand, PatronDesk, PatronFields, PatronOutput, PatronRefusal,
    ReadingHistory,
};
use elzevir_domain::patron::{Enrolment, PatronCategoryId, PatronFieldId, PatronId};
use elzevir_store::{CommandFailed, ElzevirHandler, ElzevirStore, StoreConfig};
use jiff::civil::date;

struct Library {
    dir: tempfile::TempDir,
    store: ElzevirStore,
    patrons: [PatronId; 2],
}

fn submit(store: &ElzevirStore, command: PatronCommand) -> Result<PatronOutput, PatronRefusal> {
    match store.submit(WriteCommand::Patrons(command)).unwrap().wait() {
        Ok(WriteOutput::Patrons(output)) => Ok(output),
        Err(CommandFailed::Rejected(WriteRefusal::Patrons(refusal))) => Err(refusal),
        other => panic!("unexpected {other:?}"),
    }
}

fn library() -> Library {
    let dir = tempfile::tempdir().unwrap();
    let store = ElzevirStore::open(
        &dir.path().join("elzevir.db"),
        &StoreConfig::default(),
        ElzevirHandler,
    )
    .unwrap();
    store
        .submit(WriteCommand::Catalog(CatalogCommand::NameCodes(vec![
            NamedCode {
                table: CodeTable::Branch,
                code: "B1".to_owned(),
                name: "Central".to_owned(),
            },
        ])))
        .unwrap()
        .wait()
        .unwrap();
    let branch = store.codes().unwrap().branches[0].id;
    let Ok(PatronOutput::CategoryChanged(Some(category))) = submit(
        &store,
        PatronCommand::ChangeCategory(CategoryChange::Create {
            name: "Student".to_owned(),
            enrolment: Enrolment::Months(12),
        }),
    ) else {
        panic!("no category");
    };
    let patrons = ["0001", "0002"].map(|cardnumber| {
        let Ok(PatronOutput::Added(patron)) = submit(
            &store,
            PatronCommand::Add(Box::new(PatronFields {
                cardnumber: cardnumber.to_owned(),
                surname: "Fictitious".to_owned(),
                first_name: None,
                cpf: None,
                date_of_birth: None,
                email: None,
                phone: None,
                address: None,
                branch,
                category,
                enrolled: date(2026, 1, 5),
                expires: None,
                staff_note: None,
                reading_history: ReadingHistory::NotKept,
            })),
        ) else {
            panic!("no patron");
        };
        patron
    });
    Library {
        dir,
        store,
        patrons,
    }
}

fn spec(name: &str, kind: FieldKind) -> PatronFieldSpec {
    PatronFieldSpec {
        name: name.to_owned(),
        kind,
        repeatable: true,
        unique: false,
        required: false,
        category: None,
        opac_shown: false,
        searchable: true,
    }
}

fn labels(labels: &[&str]) -> FieldKind {
    FieldKind::List(
        labels
            .iter()
            .map(|label| FieldOption {
                id: None,
                label: (*label).to_owned(),
            })
            .collect(),
    )
}

fn create(library: &Library, spec: PatronFieldSpec) -> PatronFieldId {
    let Ok(PatronOutput::FieldChanged(Some(field))) = submit(
        &library.store,
        PatronCommand::ChangeField(Box::new(FieldChange::Create(spec))),
    ) else {
        panic!("no field");
    };
    field
}

fn update(
    library: &Library,
    field: PatronFieldId,
    spec: PatronFieldSpec,
) -> Result<PatronOutput, PatronRefusal> {
    submit(
        &library.store,
        PatronCommand::ChangeField(Box::new(FieldChange::Update { field, spec })),
    )
}

/// The stored spec of `field`.
fn stored(library: &Library, field: PatronFieldId) -> PatronFieldSpec {
    let fields = library.store.fields().unwrap();
    fields
        .into_iter()
        .find(|line| line.id == field)
        .unwrap()
        .spec
}

/// Gives a patron a value directly, until the patron form writes them (part b).
fn hold(
    library: &Library,
    patron: PatronId,
    field: PatronFieldId,
    option: Option<i64>,
    text: Option<&str>,
) {
    let connection = rusqlite::Connection::open(library.dir.path().join("elzevir.db")).unwrap();
    connection
        .execute(
            "INSERT INTO patron_field_values (patron_id, field_id, option_id, text)
             VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![patron.0, field.0, option, text],
        )
        .unwrap();
}

fn option_ids(spec: &PatronFieldSpec) -> Vec<i64> {
    let FieldKind::List(options) = &spec.kind else {
        panic!("not a list");
    };
    options.iter().map(|option| option.id.unwrap().0).collect()
}

#[test]
fn a_list_field_keeps_its_options_in_order_and_renames_them_in_place() {
    let library = library();
    let field = create(&library, spec(" Shift ", labels(&["Morning", "Evening"])));
    let created = stored(&library, field);
    assert_eq!(created.name, "Shift");
    let ids = option_ids(&created);
    // Swap the labels and add one: the ids follow their option, not their label.
    let FieldKind::List(mut options) = created.kind.clone() else {
        panic!("not a list");
    };
    options.swap(0, 1);
    options[0].label = "Night".to_owned();
    options.push(FieldOption {
        id: None,
        label: "Afternoon".to_owned(),
    });
    update(
        &library,
        field,
        PatronFieldSpec {
            kind: FieldKind::List(options),
            ..created
        },
    )
    .unwrap();
    let FieldKind::List(after) = stored(&library, field).kind else {
        panic!("not a list");
    };
    let shown: Vec<_> = after
        .iter()
        .map(|option| (option.id.unwrap().0, option.label.as_str()))
        .collect();
    assert_eq!(shown[0], (ids[1], "Night"));
    assert_eq!(shown[1], (ids[0], "Morning"));
    assert_eq!(shown[2].1, "Afternoon");
    assert_eq!(shown.len(), 3);
}

#[test]
fn names_are_unique_and_the_category_must_exist() {
    let library = library();
    create(&library, spec("School", FieldKind::Text));
    let taken = submit(
        &library.store,
        PatronCommand::ChangeField(Box::new(FieldChange::Create(spec(
            "School",
            FieldKind::Date,
        )))),
    );
    assert_eq!(taken, Err(PatronRefusal::FieldNameTaken));
    let mut lost = spec("Grade", FieldKind::Text);
    lost.category = Some(PatronCategoryId(999));
    let refused = submit(
        &library.store,
        PatronCommand::ChangeField(Box::new(FieldChange::Create(lost))),
    );
    assert_eq!(refused, Err(PatronRefusal::UnknownCategory));
}

#[test]
fn held_values_guard_the_kind_the_options_and_the_delete() {
    let library = library();
    let field = create(&library, spec("Shift", labels(&["Morning", "Evening"])));
    let created = stored(&library, field);
    let ids = option_ids(&created);
    hold(&library, library.patrons[0], field, Some(ids[0]), None);
    assert_eq!(
        update(
            &library,
            field,
            PatronFieldSpec {
                kind: FieldKind::Text,
                ..created.clone()
            }
        ),
        Err(PatronRefusal::FieldKindInUse)
    );
    let FieldKind::List(options) = created.kind.clone() else {
        panic!("not a list");
    };
    // Removing the held option is refused; removing the other one is not.
    let without_held = FieldKind::List(options[1..].to_vec());
    assert_eq!(
        update(
            &library,
            field,
            PatronFieldSpec {
                kind: without_held,
                ..created.clone()
            }
        ),
        Err(PatronRefusal::FieldOptionInUse)
    );
    let without_free = FieldKind::List(options[..1].to_vec());
    update(
        &library,
        field,
        PatronFieldSpec {
            kind: without_free,
            ..created
        },
    )
    .unwrap();
    assert_eq!(
        submit(
            &library.store,
            PatronCommand::ChangeField(Box::new(FieldChange::Delete(field)))
        ),
        Err(PatronRefusal::FieldInUse)
    );
    assert_eq!(library.store.fields().unwrap()[0].patrons, 1);
}

#[test]
fn repeatable_and_unique_follow_the_values_held() {
    let library = library();
    let field = create(&library, spec("Locker", FieldKind::Text));
    let created = stored(&library, field);
    hold(&library, library.patrons[0], field, None, Some("12"));
    hold(&library, library.patrons[0], field, None, Some("13"));
    assert_eq!(
        update(
            &library,
            field,
            PatronFieldSpec {
                repeatable: false,
                ..created.clone()
            }
        ),
        Err(PatronRefusal::FieldHasRepeats)
    );
    hold(&library, library.patrons[1], field, None, Some("12"));
    assert_eq!(
        update(
            &library,
            field,
            PatronFieldSpec {
                unique: true,
                ..created.clone()
            }
        ),
        Err(PatronRefusal::FieldHasShared)
    );
    // The same patron holding a value twice is not a shared value.
    let other = create(&library, spec("Shelf", FieldKind::Text));
    hold(&library, library.patrons[0], other, None, Some("A"));
    update(
        &library,
        other,
        PatronFieldSpec {
            unique: true,
            ..stored(&library, other)
        },
    )
    .unwrap();
}

#[test]
fn a_field_keeps_its_category_from_being_deleted() {
    let library = library();
    let Ok(PatronOutput::CategoryChanged(Some(empty))) = submit(
        &library.store,
        PatronCommand::ChangeCategory(CategoryChange::Create {
            name: "Visitor".to_owned(),
            enrolment: Enrolment::Months(1),
        }),
    ) else {
        panic!("no category");
    };
    let mut limited = spec("Grade", FieldKind::Text);
    limited.category = Some(empty);
    let field = create(&library, limited);
    assert_eq!(stored(&library, field).category, Some(empty));
    let delete = || {
        submit(
            &library.store,
            PatronCommand::ChangeCategory(CategoryChange::Delete(empty)),
        )
    };
    assert_eq!(delete(), Err(PatronRefusal::CategoryInUse));
    submit(
        &library.store,
        PatronCommand::ChangeField(Box::new(FieldChange::Delete(field))),
    )
    .unwrap();
    assert!(library.store.fields().unwrap().is_empty());
    delete().unwrap();
}

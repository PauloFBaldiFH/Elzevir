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
use elzevir_app::patron_fields::{
    ExtraProblem, ExtraValue, FieldChange, FieldKind, FieldOption, PatronExtra, PatronFieldSpec,
};
use elzevir_app::patrons::{
    CategoryChange, PatronCommand, PatronDesk, PatronFields, PatronOutput, PatronQuery,
    PatronRefusal, PatronUpdate, ReadingHistory,
};
use elzevir_domain::patron::{
    Enrolment, PatronCategoryId, PatronFieldId, PatronFieldOptionId, PatronId,
};
use elzevir_store::{CommandFailed, ElzevirHandler, ElzevirStore, StoreConfig};
use jiff::civil::date;

struct Library {
    dir: tempfile::TempDir,
    store: ElzevirStore,
    category: PatronCategoryId,
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
                extra: Vec::new(),
            })),
        ) else {
            panic!("no patron");
        };
        patron
    });
    Library {
        dir,
        store,
        category,
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

/// Saves `patron` with `extra` as its additional values.
fn save(
    library: &Library,
    patron: PatronId,
    extra: Vec<PatronExtra>,
) -> Result<PatronOutput, PatronRefusal> {
    let stored = library.store.patron(patron).unwrap().unwrap();
    submit(
        &library.store,
        PatronCommand::Update(Box::new(PatronUpdate {
            patron,
            row_version: stored.row_version,
            fields: PatronFields {
                extra,
                ..stored.fields
            },
        })),
    )
}

fn value(field: PatronFieldId, value: ExtraValue) -> PatronExtra {
    PatronExtra { field, value }
}

fn text(field: PatronFieldId, text: &str) -> PatronExtra {
    value(field, ExtraValue::Text(text.to_owned()))
}

#[test]
fn values_are_saved_with_the_patron_and_shown_by_label() {
    let library = library();
    let shift = create(&library, spec("Shift", labels(&["Morning", "Evening"])));
    let evening = option_ids(&stored(&library, shift))[1];
    let born = create(&library, spec("Graduated", FieldKind::Date));
    let patron = library.patrons[0];
    save(
        &library,
        patron,
        vec![
            value(shift, ExtraValue::Option(PatronFieldOptionId(evening))),
            value(born, ExtraValue::Date(date(2020, 12, 1))),
            text(born, " "),
        ],
    )
    .unwrap();
    let read = library.store.patron(patron).unwrap().unwrap();
    assert_eq!(read.fields.extra.len(), 2);
    let lines: Vec<_> = read
        .extra_lines
        .iter()
        .map(|line| (line.name.as_str(), line.value.as_str()))
        .collect();
    assert_eq!(lines, [("Graduated", "2020-12-01"), ("Shift", "Evening")]);
    // Saving with no values takes them all away.
    save(&library, patron, Vec::new()).unwrap();
    assert!(
        library
            .store
            .patron(patron)
            .unwrap()
            .unwrap()
            .extra_lines
            .is_empty()
    );
}

#[test]
fn each_rule_of_the_values_is_refused_by_field() {
    let library = library();
    let mut single = spec("Locker", FieldKind::Text);
    single.repeatable = false;
    single.unique = true;
    let locker = create(&library, single);
    let shift = create(&library, spec("Shift", labels(&["Morning"])));
    let other = create(&library, spec("Room", labels(&["A"])));
    let room_a = option_ids(&stored(&library, other))[0];
    let [ana, bia] = library.patrons;
    let refused = |field, problem| Err(PatronRefusal::BadExtra(field, problem));
    assert_eq!(
        save(&library, ana, vec![text(locker, "12"), text(locker, "13")]),
        refused(locker, ExtraProblem::Repeated)
    );
    assert_eq!(
        save(
            &library,
            ana,
            vec![value(
                shift,
                ExtraValue::Option(PatronFieldOptionId(room_a))
            )]
        ),
        refused(shift, ExtraProblem::Invalid)
    );
    assert_eq!(
        save(
            &library,
            ana,
            vec![value(locker, ExtraValue::Date(date(2026, 1, 1)))]
        ),
        refused(locker, ExtraProblem::Invalid)
    );
    assert_eq!(
        save(&library, ana, vec![text(locker, &"x".repeat(256))]),
        refused(locker, ExtraProblem::Invalid)
    );
    save(&library, ana, vec![text(locker, "12"), text(locker, "12")]).unwrap();
    assert_eq!(
        save(&library, bia, vec![text(locker, "12")]),
        refused(locker, ExtraProblem::Taken)
    );
    // The patron who holds it saves again without tripping over their own value.
    save(&library, ana, vec![text(locker, "12")]).unwrap();
    let many = (0..=20)
        .map(|n| text(shift_text(&library), &n.to_string()))
        .collect();
    assert_eq!(
        save(&library, bia, many),
        refused(shift_text(&library), ExtraProblem::Repeated)
    );
}

/// A repeatable text field, created once.
fn shift_text(library: &Library) -> PatronFieldId {
    let found = library
        .store
        .fields()
        .unwrap()
        .into_iter()
        .find(|line| line.spec.name == "Notes");
    found.map_or_else(
        || create(library, spec("Notes", FieldKind::Text)),
        |line| line.id,
    )
}

#[test]
fn a_required_field_of_the_category_asks_for_a_value() {
    let library = library();
    let mut required = spec("School", FieldKind::Text);
    required.required = true;
    required.category = Some(library.category);
    let school = create(&library, required);
    assert_eq!(
        save(&library, library.patrons[0], Vec::new()),
        Err(PatronRefusal::BadExtra(school, ExtraProblem::Missing))
    );
    save(
        &library,
        library.patrons[0],
        vec![text(school, "Escola Fictícia")],
    )
    .unwrap();
}

#[test]
fn values_of_a_field_of_another_category_are_dropped() {
    let library = library();
    let Ok(PatronOutput::CategoryChanged(Some(visitor))) = submit(
        &library.store,
        PatronCommand::ChangeCategory(CategoryChange::Create {
            name: "Visitor".to_owned(),
            enrolment: Enrolment::Months(1),
        }),
    ) else {
        panic!("no category");
    };
    let mut limited = spec("Grade", FieldKind::Text);
    limited.category = Some(visitor);
    let grade = create(&library, limited);
    save(&library, library.patrons[0], vec![text(grade, "5")]).unwrap();
    assert!(
        library
            .store
            .patron(library.patrons[0])
            .unwrap()
            .unwrap()
            .extra_lines
            .is_empty()
    );
}

#[test]
fn the_search_finds_searchable_values_only() {
    let library = library();
    let shift = create(&library, spec("Shift", labels(&["Evening"])));
    let evening = option_ids(&stored(&library, shift))[0];
    let mut hidden = spec("Secret", FieldKind::Text);
    hidden.searchable = false;
    let secret = create(&library, hidden);
    let locker = create(&library, spec("Locker", FieldKind::Text));
    save(
        &library,
        library.patrons[0],
        vec![
            value(shift, ExtraValue::Option(PatronFieldOptionId(evening))),
            text(secret, "Hidden word"),
            text(locker, "7"),
        ],
    )
    .unwrap();
    let found = |typed: &str| {
        library
            .store
            .search(&PatronQuery::parse(typed).unwrap())
            .unwrap()
            .into_iter()
            .map(|row| row.id)
            .collect::<Vec<_>>()
    };
    assert_eq!(found("EVEN"), [library.patrons[0]]);
    assert_eq!(found("7"), [library.patrons[0]]);
    assert!(found("Hidden").is_empty());
}

#[test]
fn the_export_carries_the_values_and_anonymizing_deletes_them() {
    let library = library();
    let locker = create(&library, spec("Locker", FieldKind::Text));
    let patron = library.patrons[0];
    save(&library, patron, vec![text(locker, "12")]).unwrap();
    let export = library.store.export(patron).unwrap().unwrap();
    assert_eq!(
        export["additional_fields"],
        serde_json::json!([{"field": "Locker", "value": "12"}])
    );
    submit(&library.store, PatronCommand::Anonymize(patron)).unwrap();
    assert_eq!(library.store.fields().unwrap()[0].patrons, 0);
}

#[test]
fn a_second_save_from_an_old_form_is_stale() {
    let library = library();
    let locker = create(&library, spec("Locker", FieldKind::Text));
    let patron = library.patrons[0];
    let read = library.store.patron(patron).unwrap().unwrap();
    save(&library, patron, vec![text(locker, "12")]).unwrap();
    let late = submit(
        &library.store,
        PatronCommand::Update(Box::new(PatronUpdate {
            patron,
            row_version: read.row_version,
            fields: PatronFields {
                extra: vec![text(locker, "13")],
                ..read.fields
            },
        })),
    );
    assert_eq!(late, Err(PatronRefusal::Stale));
    let lines = library.store.patron(patron).unwrap().unwrap().extra_lines;
    assert_eq!(lines[0].value, "12");
}

#[test]
fn the_opac_account_shows_only_the_fields_marked_for_it() {
    use elzevir_app::patron_account::PatronAccounts;
    let library = library();
    let mut shown = spec("Shift", FieldKind::Text);
    shown.opac_shown = true;
    let shift = create(&library, shown);
    let note = create(&library, spec("Staff only", FieldKind::Text));
    let patron = library.patrons[0];
    save(
        &library,
        patron,
        vec![text(shift, "Evening"), text(note, "Internal")],
    )
    .unwrap();
    let account = library.store.account(patron).unwrap().unwrap();
    let lines: Vec<_> = account
        .extra_lines
        .iter()
        .map(|line| (line.name.as_str(), line.value.as_str()))
        .collect();
    assert_eq!(lines, [("Shift", "Evening")]);
}

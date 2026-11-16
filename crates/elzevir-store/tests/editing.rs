// SPDX-License-Identifier: GPL-3.0-or-later
//! Editing a stored Work or Instance in place (docs/specs/catalogo/edicao.md).
#![expect(
    clippy::unwrap_used,
    clippy::panic,
    reason = "integration test: helper functions fail the test on error"
)]

use elzevir_app::codes::{CodeTable, NamedCode};
use std::collections::BTreeSet;

use elzevir_app::catalog::{
    CatalogCommand, CatalogOutput, CatalogReader, CatalogRefusal, InstanceChange, InstanceUpdate,
    InstanceView, NewContribution, NewIdentifier, NewSubject, WorkChange, WorkUpdate,
};
use elzevir_app::commands::{WriteCommand, WriteOutput, WriteRefusal};
use elzevir_app::export::{ExportOptions, export_marc21, item_codes};
use elzevir_app::import::{ImportOptions, import_marc21};
use elzevir_domain::catalog::{IdentifierKind, InstanceId, SubjectKind, WorkId};
use elzevir_marc::{Limits, MarcReader};
use elzevir_store::{CommandFailed, ElzevirHandler, ElzevirStore, StoreConfig};

const STAFF_EXPORT: &[u8] =
    include_bytes!("../../elzevir-app/tests/data/koha-staff-bibs-1-200.mrc");

fn imported(dir: &tempfile::TempDir) -> ElzevirStore {
    let store = ElzevirStore::open(
        &dir.path().join("elzevir.db"),
        &StoreConfig::default(),
        ElzevirHandler,
    )
    .unwrap();
    let codes = item_codes(STAFF_EXPORT, Limits::default()).unwrap();
    let branches = codes.branches.into_iter().map(|code| {
        CatalogCommand::NameCodes(vec![NamedCode {
            table: CodeTable::Branch,
            name: code.clone(),
            code,
        }])
    });
    let types = codes.item_types.into_iter().map(|code| {
        CatalogCommand::NameCodes(vec![NamedCode {
            table: CodeTable::ItemType,
            name: code.clone(),
            code,
        }])
    });
    for command in branches.chain(types) {
        run(&store, command).unwrap();
    }
    import_marc21(STAFF_EXPORT, &store, &ImportOptions::default()).unwrap();
    store
}

fn run(
    store: &ElzevirStore,
    command: CatalogCommand,
) -> Result<CatalogOutput, CommandFailed<WriteRefusal>> {
    store
        .submit(WriteCommand::Catalog(command))
        .unwrap()
        .wait()
        .map(|output| {
            let WriteOutput::Catalog(output) = output else {
                panic!("unexpected output {output:?}");
            };
            output
        })
}

fn view(store: &ElzevirStore, id: i64) -> InstanceView {
    store.instance(InstanceId(id)).unwrap().unwrap()
}

fn update_work(view: &InstanceView, changes: Vec<WorkChange>) -> CatalogCommand {
    CatalogCommand::UpdateWork(Box::new(WorkUpdate {
        work: view.work,
        row_version: view.work_version,
        changes,
    }))
}

fn update_instance(view: &InstanceView, changes: Vec<InstanceChange>) -> CatalogCommand {
    CatalogCommand::UpdateInstance(Box::new(InstanceUpdate {
        instance: view.id,
        row_version: view.version,
        changes,
    }))
}

fn export(store: &ElzevirStore) -> Vec<u8> {
    let mut out = Vec::new();
    let report = export_marc21(store, &mut out, &ExportOptions::default()).unwrap();
    assert!(report.failures.is_empty(), "{:?}", report.failures);
    out
}

/// Each record of an ISO 2709 file as its fields: tag and raw bytes, in order.
fn fields(file: &[u8]) -> Vec<Vec<(String, Vec<u8>)>> {
    let mut reader = MarcReader::new(file, Limits::default());
    let mut records = Vec::new();
    while let Some(record) = reader.next_record() {
        let record = record.unwrap();
        records.push(
            record
                .fields()
                .map(|field| (field.tag().to_string(), field.raw().to_vec()))
                .collect(),
        );
    }
    records
}

fn count(store: &ElzevirStore, sql: &str) -> i64 {
    store
        .readers()
        .read(|c| c.query_row(sql, [], |row| row.get(0)))
        .unwrap()
}

#[test]
fn an_edit_changes_only_the_edited_fields_of_the_exported_record() {
    let dir = tempfile::tempdir().unwrap();
    let store = imported(&dir);
    let before = view(&store, 1);
    run(
        &store,
        update_work(
            &before,
            vec![WorkChange::Summary(Some("Resumo revisto.".to_owned()))],
        ),
    )
    .unwrap();
    let before = view(&store, 1);
    run(
        &store,
        update_instance(
            &before,
            vec![InstanceChange::Publisher(Some(
                "Editora Revista".to_owned(),
            ))],
        ),
    )
    .unwrap();

    let original = fields(STAFF_EXPORT);
    let edited = fields(&export(&store));
    assert_eq!(original.len(), edited.len());
    // Every other record goes out byte for byte.
    assert_eq!(original.get(1..), edited.get(1..));
    // In the edited one, only the summary and the publication fields moved.
    let touched = |field: &&(String, Vec<u8>)| ["520", "260", "264"].contains(&field.0.as_str());
    let untouched = |record: &Vec<(String, Vec<u8>)>| {
        record
            .iter()
            .filter(|field| !touched(field))
            .cloned()
            .collect::<Vec<_>>()
    };
    assert_eq!(untouched(&original[0]), untouched(&edited[0]));
    let text = |record: &Vec<(String, Vec<u8>)>| {
        record
            .iter()
            .filter(touched)
            .map(|field| String::from_utf8_lossy(&field.1).into_owned())
            .collect::<String>()
    };
    assert!(text(&edited[0]).contains("Resumo revisto."));
    assert!(text(&edited[0]).contains("Editora Revista"));
}

#[test]
fn an_edit_read_before_another_one_is_refused_and_writes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let store = imported(&dir);
    let read = view(&store, 1);
    let first = update_work(&read, vec![WorkChange::Title("Primeira".to_owned())]);
    let second = update_work(&read, vec![WorkChange::Title("Segunda".to_owned())]);
    assert_eq!(
        run(&store, first).unwrap(),
        CatalogOutput::WorkUpdated {
            work: read.work,
            row_version: read.work_version + 1,
        }
    );
    let effects = count(&store, "SELECT count(*) FROM effect_outbox");
    let logged = count(&store, "SELECT count(*) FROM audit_log");
    assert_eq!(
        run(&store, second),
        Err(CommandFailed::Rejected(WriteRefusal::Catalog(
            CatalogRefusal::Stale {
                read: read.work_version,
                current: read.work_version + 1,
            }
        )))
    );
    assert_eq!(view(&store, 1).work_title, "Primeira");
    assert_eq!(count(&store, "SELECT count(*) FROM effect_outbox"), effects);
    assert_eq!(count(&store, "SELECT count(*) FROM audit_log"), logged);

    // The same holds for Instances: the record moved with the Work edit.
    let stale = update_instance(&read, vec![InstanceChange::Date(Some("2001".to_owned()))]);
    assert_eq!(
        run(&store, stale),
        Err(CommandFailed::Rejected(WriteRefusal::Catalog(
            CatalogRefusal::Stale {
                read: read.version,
                current: read.version + 1,
            }
        )))
    );
}

#[test]
fn a_work_edit_moves_the_version_of_its_record_and_queues_a_reindex() {
    let dir = tempfile::tempdir().unwrap();
    let store = imported(&dir);
    let read = view(&store, 2);
    let reindexed = || {
        count(
            &store,
            "SELECT count(*) FROM effect_outbox
             WHERE kind = 'catalog.reindex_instance' AND payload = '{\"instance\":2}'",
        )
    };
    let queued = reindexed();
    run(
        &store,
        update_work(
            &read,
            vec![
                WorkChange::Languages(vec!["eng".to_owned()]),
                WorkChange::Audience(Some("Adulto".to_owned())),
            ],
        ),
    )
    .unwrap();
    let after = view(&store, 2);
    assert_eq!(after.version, read.version + 1);
    assert_eq!(after.languages, ["eng"]);
    assert_eq!(after.audience.as_deref(), Some("Adulto"));
    assert_eq!(reindexed(), queued + 1);
    let kind = count(
        &store,
        "SELECT count(*) FROM audit_log WHERE kind = 'catalog.update_work'",
    );
    assert_eq!(kind, 1);
}

#[test]
fn an_identifier_edit_moves_the_match_keys() {
    let dir = tempfile::tempdir().unwrap();
    let store = imported(&dir);
    let read = view(&store, 3);
    let biblionumber = "SELECT count(*) FROM match_keys WHERE instance_id = 3 AND rule = 3";
    let kept = count(&store, biblionumber);
    run(
        &store,
        update_instance(
            &read,
            vec![InstanceChange::Identifiers(vec![NewIdentifier {
                kind: IdentifierKind::Isbn,
                value: "85-359-0277-5".to_owned(),
                qualifier: None,
            }])],
        ),
    )
    .unwrap();
    let keys = store
        .readers()
        .read(|c| {
            let mut statement = c.prepare(
                "SELECT key FROM match_keys WHERE instance_id = 3 AND rule <> 3 ORDER BY key",
            )?;
            statement
                .query_map([], |row| row.get::<_, String>(0))?
                .collect::<Result<BTreeSet<_>, _>>()
        })
        .unwrap();
    assert_eq!(keys, ["9788535902778".to_owned()].into());
    assert_eq!(count(&store, biblionumber), kept);
}

#[test]
fn editing_what_does_not_exist_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let store = imported(&dir);
    let missing = CatalogCommand::UpdateWork(Box::new(WorkUpdate {
        work: WorkId(9999),
        row_version: 1,
        changes: vec![WorkChange::Summary(None)],
    }));
    assert_eq!(
        run(&store, missing),
        Err(CommandFailed::Rejected(WriteRefusal::Catalog(
            CatalogRefusal::UnknownWork(WorkId(9999))
        )))
    );
    let missing = CatalogCommand::UpdateInstance(Box::new(InstanceUpdate {
        instance: InstanceId(9999),
        row_version: 1,
        changes: Vec::new(),
    }));
    assert_eq!(
        run(&store, missing),
        Err(CommandFailed::Rejected(WriteRefusal::Catalog(
            CatalogRefusal::UnknownInstance(InstanceId(9999))
        )))
    );
}

#[test]
fn an_edit_from_the_screen_records_who_made_it() {
    use elzevir_app::editing::{CatalogEditor, Edit, EditFailed};
    use elzevir_app::staff::StaffId;

    let dir = tempfile::tempdir().unwrap();
    let store = imported(&dir);
    let first = view(&store, 1);
    let edit = Edit::Work(WorkUpdate {
        work: first.work,
        row_version: first.work_version,
        changes: vec![WorkChange::Summary(Some("Resumo novo.".to_owned()))],
    });
    let output = store.apply(StaffId(7), edit.clone()).unwrap();
    assert!(matches!(output, CatalogOutput::WorkUpdated { .. }));
    assert_eq!(
        count(
            &store,
            "SELECT actor FROM audit_log WHERE kind = 'catalog.update_work'"
        ),
        7
    );
    // Imports have no actor.
    assert!(count(&store, "SELECT count(*) FROM audit_log WHERE actor IS NULL") > 0);
    // The same edit again was read at the old version.
    assert!(matches!(
        store.apply(StaffId(7), edit),
        Err(EditFailed::Refused(CatalogRefusal::Stale { .. }))
    ));
}

#[test]
fn suggestions_find_existing_agents_and_subjects_by_the_start_of_their_name() {
    use elzevir_app::suggestions::AuthoritySuggestions;

    let dir = tempfile::tempdir().unwrap();
    let store = imported(&dir);
    let agents = store.agents("oliveira, c", 10).unwrap();
    assert!(!agents.is_empty());
    assert!(
        agents
            .iter()
            .all(|agent| agent.heading.to_lowercase().starts_with("oliveira, c"))
    );
    let subjects = store.subjects("Exporta", 10).unwrap();
    assert!(
        subjects
            .iter()
            .any(|subject| subject.heading == "Exportação")
    );
    assert!(store.agents("o", 3).unwrap().len() <= 3);
    // LIKE wildcards in what was typed match only themselves.
    assert!(store.agents("%", 10).unwrap().is_empty());
    assert!(store.subjects("_x", 10).unwrap().is_empty());
    // The lookup uses the index of migration 10 instead of reading the whole table.
    let plan: String = store
        .readers()
        .read(|c| {
            c.query_row(
                "EXPLAIN QUERY PLAN SELECT heading, kind FROM agents
                 WHERE heading LIKE 'Oli%' ESCAPE '\\' ORDER BY heading COLLATE NOCASE, kind",
                [],
                |row| row.get::<_, String>(3),
            )
        })
        .unwrap();
    assert!(plan.contains("agents_by_heading"), "{plan}");
}

#[test]
fn a_classes_edit_replaces_the_classes_keeps_the_statements_and_refuses_others() {
    use elzevir_app::catalog::{CatalogRefusal, ClassChoice, StatementReader};
    use elzevir_domain::statement::{EntityKind, EntityRef};

    let choose = |names: &[&str]| {
        names
            .iter()
            .map(|name| ClassChoice {
                class: (*name).to_owned(),
            })
            .collect::<Vec<_>>()
    };

    let dir = tempfile::tempdir().unwrap();
    let store = imported(&dir);
    let read = view(&store, 1);
    let work = EntityRef {
        entity: EntityKind::Work,
        id: read.work.0,
    };
    let before = store.description(work).unwrap();
    assert_eq!(before.classes, ["Text", "Monograph"]);
    run(
        &store,
        update_work(&read, vec![WorkChange::Classes(choose(&["NotatedMusic"]))]),
    )
    .unwrap();
    let after = store.description(work).unwrap();
    assert_eq!(after.classes, ["NotatedMusic"]);
    assert_eq!(after.statements, before.statements);
    let refused = run(
        &store,
        update_work(
            &view(&store, 1),
            vec![WorkChange::Classes(choose(&["Print"]))],
        ),
    )
    .unwrap_err();
    assert!(
        matches!(
            refused,
            CommandFailed::Rejected(WriteRefusal::Catalog(CatalogRefusal::BadStatement(_)))
        ),
        "{refused:?}"
    );
}

#[test]
fn a_subject_list_edit_keeps_what_the_kept_subdivisions_name() {
    let dir = tempfile::tempdir().unwrap();
    let store = imported(&dir);
    // The twelfth record has `650 $aDiscursos parlamentares$zBrasil`.
    let before = view(&store, 12);
    // The form sends each heading as text, without the kinds of its subdivisions.
    let mut subjects: Vec<NewSubject> = before
        .subjects
        .iter()
        .map(|(kind, heading)| NewSubject {
            kind: *kind,
            heading: heading.clone(),
            thesaurus: None,
            subdivisions: Vec::new(),
            name_parts: Vec::new(),
            identifiers: Vec::new(),
        })
        .collect();
    subjects.push(NewSubject {
        kind: SubjectKind::Topic,
        heading: "Retórica -- Brasil".to_owned(),
        thesaurus: None,
        subdivisions: Vec::new(),
        name_parts: Vec::new(),
        identifiers: Vec::new(),
    });
    run(
        &store,
        update_work(&before, vec![WorkChange::Subjects(subjects)]),
    )
    .unwrap();
    let edited = fields(&export(&store));
    let subjects: Vec<String> = edited[11]
        .iter()
        .filter(|field| field.0 == "650")
        .map(|field| String::from_utf8_lossy(&field.1).into_owned())
        .collect();
    assert!(
        subjects
            .iter()
            .any(|f| f.contains("Discursos parlamentares\u{1f}zBrasil"))
    );
    // A new heading typed as text has topics only.
    assert!(subjects.iter().any(|f| f.contains("Retórica\u{1f}xBrasil")));
}

#[test]
fn a_contribution_typed_as_text_keeps_the_parts_of_its_agent() {
    let dir = tempfile::tempdir().unwrap();
    let store = imported(&dir);
    // The twelfth record has `100 $aFigueirêdo, Argemiro de,$d1901-1982`.
    let before = view(&store, 12);
    let contributions: Vec<NewContribution> = before
        .contributors
        .iter()
        .enumerate()
        .map(|(position, (kind, heading))| NewContribution {
            kind: *kind,
            heading: heading.clone(),
            role: None,
            primary: position == 0,
            name_parts: Vec::new(),
            role_term: None,
            identifiers: Vec::new(),
        })
        .rev()
        .collect();
    run(
        &store,
        update_work(&before, vec![WorkChange::Contributions(contributions)]),
    )
    .unwrap();
    let edited = fields(&export(&store));
    let names: String = edited[11]
        .iter()
        .filter(|field| ["100", "700"].contains(&field.0.as_str()))
        .map(|field| String::from_utf8_lossy(&field.1).into_owned())
        .collect();
    assert!(
        names.contains("Figueirêdo, Argemiro de,\u{1f}d1901-1982"),
        "{names}"
    );
}

#[test]
fn a_contribution_list_edit_keeps_the_relator_terms_and_authority_links() {
    let dir = tempfile::tempdir().unwrap();
    let store = imported(&dir);
    // The nineteenth record has `100 $aGinzberg, Eli,$d1911-2002,$eautor$1…$0…$4aut`
    // and `700 $aEichner, Alfred S.,$d1937-1988,$eautor$1…$0…`, whose role is the term.
    let before = view(&store, 19);
    // The form sends the agent and the role, not the term or the links.
    let contributions: Vec<NewContribution> = before
        .contributors
        .iter()
        .enumerate()
        .map(|(position, (kind, heading))| NewContribution {
            kind: *kind,
            heading: heading.clone(),
            role: Some(if position == 0 { "aut" } else { "autor" }.to_owned()),
            primary: position == 0,
            name_parts: Vec::new(),
            role_term: None,
            identifiers: Vec::new(),
        })
        .collect();
    run(
        &store,
        update_work(&before, vec![WorkChange::Contributions(contributions)]),
    )
    .unwrap();
    let edited = fields(&export(&store));
    let names: Vec<String> = edited[18]
        .iter()
        .filter(|field| ["100", "700"].contains(&field.0.as_str()))
        .map(|field| String::from_utf8_lossy(&field.1).into_owned())
        .collect();
    assert_eq!(names.len(), 2, "{names:?}");
    assert!(
        names[0].contains(
            "\u{1f}eautor\u{1f}1http://viaf.org/viaf/108304418\u{1f}1https://www.wikidata.org/wiki/Q5360231\u{1f}0http://id.loc.gov/authorities/names/n79039850\u{1f}4aut"
        ),
        "{names:?}"
    );
    assert!(
        names[1].contains("\u{1f}eautor\u{1f}1http://viaf.org/viaf/94384300"),
        "{names:?}"
    );
}

#[test]
fn a_subject_list_edit_keeps_the_parts_and_links_of_a_name() {
    let dir = tempfile::tempdir().unwrap();
    let store = imported(&dir);
    // The twentieth record has `600 14 $aQuércia, Orestes,$d1938-2010$1…$1…$0…`.
    let before = view(&store, 20);
    let mut subjects: Vec<NewSubject> = before
        .subjects
        .iter()
        .map(|(kind, heading)| NewSubject {
            kind: *kind,
            heading: heading.clone(),
            thesaurus: None,
            subdivisions: Vec::new(),
            name_parts: Vec::new(),
            identifiers: Vec::new(),
        })
        .collect();
    subjects.push(NewSubject {
        kind: SubjectKind::Organization,
        heading: "Brasil. Senado Federal".to_owned(),
        thesaurus: None,
        subdivisions: Vec::new(),
        name_parts: Vec::new(),
        identifiers: Vec::new(),
    });
    run(
        &store,
        update_work(&before, vec![WorkChange::Subjects(subjects)]),
    )
    .unwrap();
    let edited = fields(&export(&store));
    let names: Vec<String> = edited[19]
        .iter()
        .filter(|field| ["600", "610"].contains(&field.0.as_str()))
        .map(|field| String::from_utf8_lossy(&field.1).into_owned())
        .collect();
    assert!(
        names.iter().any(|f| f.contains(
            "\u{1f}aQuércia, Orestes,\u{1f}d1938-2010\u{1f}1http://viaf.org/viaf/60476955"
        )),
        "{names:?}"
    );
    // A name typed in the form goes out as its kind, whole in `$a`.
    assert!(
        names.iter().any(|f| f == "24\u{1f}aBrasil. Senado Federal"),
        "{names:?}"
    );
}

#[test]
fn a_property_edit_replaces_its_statements_in_place_and_keeps_the_others() {
    use elzevir_app::catalog::StatementReader;
    use elzevir_domain::statement::{EntityKind, EntityRef, Statement, Value};

    let dir = tempfile::tempdir().unwrap();
    let store = imported(&dir);
    let read = view(&store, 1);
    let work = EntityRef {
        entity: EntityKind::Work,
        id: read.work.0,
    };
    let before = store.statements(work).unwrap();
    assert!(!before.is_empty());
    let first = before[0].property.clone();
    let award = |text: &str| Statement {
        property: "awards".to_owned(),
        value: Value::Literal {
            text: text.to_owned(),
            language: None,
        },
    };
    let statements = |list: Vec<Statement>| WorkChange::Statements {
        property: "awards".to_owned(),
        statements: list,
    };
    run(
        &store,
        update_work(
            &read,
            vec![statements(vec![award("Jabuti"), award("APCA")])],
        ),
    )
    .unwrap();
    let after = store.statements(work).unwrap();
    assert_eq!(after[..before.len()], before[..]);
    assert_eq!(after[before.len()..], [award("Jabuti"), award("APCA")]);
    // Replaced where they were, and the version moved.
    let read = view(&store, 1);
    run(
        &store,
        update_work(&read, vec![statements(vec![award("Prêmio")])]),
    )
    .unwrap();
    let after = store.statements(work).unwrap();
    assert_eq!(after.last(), Some(&award("Prêmio")));
    assert_eq!(after[0].property, first);
    // What the store refuses writes nothing.
    let read = view(&store, 1);
    let wrong = WorkChange::Statements {
        property: "awards".to_owned(),
        statements: vec![Statement {
            property: "credits".to_owned(),
            value: Value::Literal {
                text: "x".to_owned(),
                language: None,
            },
        }],
    };
    assert!(matches!(
        run(&store, update_work(&read, vec![wrong])),
        Err(CommandFailed::Rejected(WriteRefusal::Catalog(
            CatalogRefusal::BadStatement(_)
        )))
    ));
    assert_eq!(store.statements(work).unwrap(), after);
}

#[test]
fn an_instance_property_edit_keeps_the_typed_parts_and_the_other_statements() {
    use elzevir_app::catalog::StatementReader;
    use elzevir_domain::statement::{EntityKind, EntityRef, Statement, Value};

    let dir = tempfile::tempdir().unwrap();
    let store = imported(&dir);
    let read = view(&store, 1);
    let instance = EntityRef {
        entity: EntityKind::Instance,
        id: read.id.0,
    };
    let before = store.statements(instance).unwrap();
    let size = Statement {
        property: "dimensions".to_owned(),
        value: Value::Literal {
            text: "21 cm".to_owned(),
            language: None,
        },
    };
    run(
        &store,
        update_instance(
            &read,
            vec![InstanceChange::Statements {
                property: "dimensions".to_owned(),
                statements: vec![size.clone()],
            }],
        ),
    )
    .unwrap();
    let after = store.statements(instance).unwrap();
    let others = |list: &[Statement]| {
        list.iter()
            .filter(|statement| statement.property != "dimensions")
            .cloned()
            .collect::<Vec<_>>()
    };
    assert_eq!(others(&after), others(&before));
    assert!(after.contains(&size));
    let again = view(&store, 1);
    assert_eq!(again.title, read.title);
    assert!(again.version > read.version);
}

#[test]
fn an_item_property_edit_replaces_its_statements_and_moves_the_version() {
    use elzevir_app::catalog::{PropertyEdit, PropertyStatements, StatementReader};
    use elzevir_domain::statement::{EntityKind, EntityRef, Statement, Value};

    let dir = tempfile::tempdir().unwrap();
    let store = imported(&dir);
    let id: i64 = count(&store, "SELECT MIN(id) FROM items");
    let item = EntityRef {
        entity: EntityKind::Item,
        id,
    };
    let version = store.version(item).unwrap().unwrap();
    let place = Statement {
        property: "physicalLocation".to_owned(),
        value: Value::Literal {
            text: "Estante 4".to_owned(),
            language: None,
        },
    };
    let edit = |row_version| {
        CatalogCommand::EditStatements(Box::new(PropertyEdit {
            subject: item,
            row_version,
            changes: vec![PropertyStatements {
                property: "physicalLocation".to_owned(),
                statements: vec![place.clone()],
            }],
        }))
    };
    let output = run(&store, edit(version)).unwrap();
    assert!(matches!(output,
        CatalogOutput::StatementsSet { row_version, .. } if row_version == version + 1));
    assert!(store.statements(item).unwrap().contains(&place));
    assert!(matches!(
        run(&store, edit(version)),
        Err(CommandFailed::Rejected(WriteRefusal::Catalog(
            CatalogRefusal::Stale { .. }
        )))
    ));
    let unknown = EntityRef {
        entity: EntityKind::Hub,
        id: 9999,
    };
    assert_eq!(store.version(unknown).unwrap(), None);
}

#[test]
fn a_relations_edit_replaces_the_related_works() {
    use elzevir_app::catalog::NewWorkRelation;
    use elzevir_domain::catalog::WorkRelationKind;

    let dir = tempfile::tempdir().unwrap();
    let store = imported(&dir);
    let read = view(&store, 1);
    let other = view(&store, 2).work;
    let relation = |kind, work| NewWorkRelation { kind, work };
    run(
        &store,
        update_work(
            &read,
            vec![WorkChange::Relations(vec![relation(
                WorkRelationKind::TranslationOf,
                other,
            )])],
        ),
    )
    .unwrap();
    let sql = format!(
        "SELECT COUNT(*) FROM work_relations WHERE work_id = {} AND related_work_id = {}",
        read.work.0, other.0
    );
    assert_eq!(count(&store, &sql), 1);
    let read = view(&store, 1);
    run(
        &store,
        update_work(&read, vec![WorkChange::Relations(Vec::new())]),
    )
    .unwrap();
    assert_eq!(count(&store, &sql), 0);
    // A Work does not relate to itself, and nothing is written.
    let read = view(&store, 1);
    assert!(
        run(
            &store,
            update_work(
                &read,
                vec![WorkChange::Relations(vec![relation(
                    WorkRelationKind::PartOf,
                    read.work,
                )])],
            ),
        )
        .is_err()
    );
}

#[test]
fn the_history_of_a_work_and_an_instance_says_who_changed_what_and_when() {
    use elzevir_app::commands::StaffCommand;
    use elzevir_app::editing::{CatalogEditor, Edit};
    use elzevir_app::history::{Change, HistoryReader, Part, entry};
    use elzevir_domain::statement::{EntityKind, EntityRef};

    let dir = tempfile::tempdir().unwrap();
    let store = imported(&dir);
    let WriteOutput::StaffCreated(ana) = store
        .submit(WriteCommand::Staff(StaffCommand::Create {
            login: "ana".to_owned(),
            password_hash: "x".to_owned(),
            permissions: BTreeSet::new(),
        }))
        .unwrap()
        .wait()
        .unwrap()
    else {
        panic!("no account");
    };
    let first = view(&store, 1);
    store
        .apply(
            ana,
            Edit::Work(WorkUpdate {
                work: first.work,
                row_version: first.work_version,
                changes: vec![
                    WorkChange::Summary(Some("Resumo novo.".to_owned())),
                    WorkChange::Languages(vec!["por".to_owned()]),
                ],
            }),
        )
        .unwrap();
    let first = view(&store, 1);
    store
        .apply(
            ana,
            Edit::Instance(InstanceUpdate {
                instance: first.id,
                row_version: first.version,
                changes: vec![InstanceChange::Edition(Some("2. ed.".to_owned()))],
            }),
        )
        .unwrap();
    let history = |entity, id| -> Vec<_> {
        store
            .history(EntityRef { entity, id }, 10)
            .unwrap()
            .into_iter()
            .map(entry)
            .collect()
    };
    let work = history(EntityKind::Work, first.work.0);
    assert_eq!(work.len(), 2, "{work:?}");
    assert_eq!(work[1].change, Change::Imported);
    assert_eq!(work[0].actor.as_deref(), Some("ana"));
    assert_eq!(
        work[0].change,
        Change::Edited(vec![
            Part::Field("Summary".to_owned()),
            Part::Field("Languages".to_owned())
        ])
    );
    let at = work[0].at.clone().unwrap();
    assert_eq!(
        (at.len(), at.get(10..11), at.get(19..)),
        (20, Some("T"), Some("Z"))
    );
    // Newest first: the edit, then the import, which has no account.
    let instance = history(EntityKind::Instance, 1);
    assert_eq!(
        instance
            .iter()
            .map(|line| line.change.clone())
            .collect::<Vec<_>>(),
        [
            Change::Edited(vec![Part::Field("Edition".to_owned())]),
            Change::Imported
        ]
    );
    assert_eq!(instance[1].actor, None);
    assert_eq!(history(EntityKind::Instance, 2).len(), 1);
    assert_eq!(history(EntityKind::Instance, 9999), []);
}

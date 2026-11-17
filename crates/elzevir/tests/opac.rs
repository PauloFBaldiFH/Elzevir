// SPDX-License-Identifier: GPL-3.0-or-later
//! The OPAC over the real store and index, with the 200 records of the staff export
//! (docs/specs/opac/busca-e-registro.md).
#![expect(
    clippy::unwrap_used,
    reason = "integration test: helper functions fail the test on error"
)]

mod common;

use std::num::NonZeroUsize;
use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use elzevir_app::catalog::{
    CatalogCommand, CatalogReader, InstanceChange, InstanceUpdate, ItemData, ItemStatus, NewItem,
};
use elzevir_app::commands::WriteCommand;
use elzevir_app::export::{RecordFormat, export_record};
use elzevir_app::memory::{MemoryProbe, MemoryReport};
use elzevir_app::parallelism::Parallelism;
use elzevir_app::search::{IndexOptions, SearchIndex, SearchQuery, SearchScope, index_pending};
use elzevir_marc::{Limits, MarcReader};
use elzevir_search::TantivyIndex;
use elzevir_store::ElzevirStore;
use elzevir_web::{AppState, OpacPorts, StaffPorts, WebConfig, router};
use elzevir_web_templates::I18n;
use http_body_util::BodyExt;
use tower::ServiceExt;

use common::imported;

struct NoCounters;

impl MemoryProbe for NoCounters {
    fn report(&self) -> MemoryReport {
        MemoryReport {
            allocator: "system",
            counters: None,
        }
    }
}

fn options() -> IndexOptions {
    IndexOptions {
        parallelism: Parallelism::Serial,
        batch: NonZeroUsize::new(1024).unwrap(),
    }
}

fn app(store: &Arc<ElzevirStore>, index: &Arc<TantivyIndex>) -> Router {
    router(AppState::new(
        Arc::clone(store) as _,
        Arc::new(NoCounters),
        OpacPorts {
            catalog: Arc::clone(store) as _,
            index: Arc::clone(index) as _,
        },
        StaffPorts {
            listing: Arc::clone(store) as _,
            records: Arc::clone(store) as _,
            editor: Arc::clone(store) as _,
            suggestions: Arc::clone(store) as _,
            items: Arc::clone(store) as _,
            statements: Arc::clone(store) as _,
            history: Arc::clone(store) as _,
            authorities: Arc::clone(store) as _,
            work_lists: Arc::clone(store) as _,
        },
        I18n::english().unwrap(),
        WebConfig::default(),
    ))
}

/// One request, on a runtime of its own: the store's writer is waited for synchronously,
/// which cannot happen inside a runtime, so the tests stay synchronous.
fn get(app: &Router, uri: &str) -> (StatusCode, String) {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    runtime.block_on(async {
        let response = app
            .clone()
            .oneshot(Request::get(uri).body(Body::empty()).unwrap())
            .await
            .unwrap();
        let status = response.status();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        (status, String::from_utf8(bytes.to_vec()).unwrap())
    })
}

#[test]
fn search_then_record_then_a_new_item_shows_up() {
    let (_dir, store) = imported();
    let store = Arc::new(store);
    let index = Arc::new(TantivyIndex::in_memory(Parallelism::Serial).unwrap());
    index_pending(&*store, &*store, &*index, &options()).unwrap();
    let app = app(&store, &index);

    let (status, results) = get(&app, "/search?q=anuario+estatistico");
    assert_eq!(status, StatusCode::OK);
    assert!(results.contains("Perfil - Anuário Estatístico"));
    assert!(results.contains("available"));
    let instance = index
        .search(
            &SearchQuery::words("anuario estatistico"),
            SearchScope::Staff,
            0,
            1,
        )
        .unwrap()
        .hits[0]
        .instance;

    let uri = format!("/record/{}", instance.0);
    let (status, record) = get(&app, &uri);
    assert_eq!(status, StatusCode::OK);
    assert!(record.contains("310.62 P94"));
    let copies = record.matches("<tr>").count();

    // The edge codes of a branch and a policy: AddItem carries Items as MARC does.
    let edge = store.codes().unwrap().edge;
    let code_of = |table| {
        edge.iter()
            .find(|entry| entry.table == table)
            .unwrap()
            .code
            .clone()
    };
    let branch = code_of(elzevir_app::codes::CodeTable::Branch);
    let item_type = code_of(elzevir_app::codes::CodeTable::ItemType);
    store
        .submit(WriteCommand::Catalog(CatalogCommand::AddItem(Box::new(
            NewItem {
                instance,
                item: ItemData {
                    barcode: Some("OPAC-TEST-1".to_owned()),
                    inventory_number: None,
                    home_branch: branch.clone(),
                    holding_branch: branch,
                    item_type,
                    call_number: Some("310.62 P94 ex.9".to_owned()),
                    location: None,
                    collection: None,
                    price: None,
                    replacement_price: None,
                    copy_number: None,
                    status: ItemStatus {
                        lost: 1,
                        ..ItemStatus::default()
                    },
                    acquired_on: None,
                },
            },
        ))))
        .unwrap()
        .wait()
        .unwrap();
    let (_, after) = get(&app, &uri);
    assert_eq!(after.matches("<tr>").count(), copies + 1);
    assert!(after.contains("310.62 P94 ex.9"));
    assert!(after.contains("Lost"));
}

#[test]
fn a_record_shows_its_other_titles() {
    let (_dir, store) = imported();
    let store = Arc::new(store);
    let index = Arc::new(TantivyIndex::in_memory(Parallelism::Serial).unwrap());
    index_pending(&*store, &*store, &*index, &options()).unwrap();
    let app = app(&store, &index);
    let instance = index
        .search(
            &SearchQuery::words("textos perdidos"),
            SearchScope::Staff,
            0,
            1,
        )
        .unwrap()
        .hits[0]
        .instance;
    let (status, record) = get(&app, &format!("/record/{}", instance.0));
    assert_eq!(status, StatusCode::OK);
    assert!(record.contains("Other titles"));
    assert!(record.contains("<dd>Textos perdidos</dd>"));
}

#[test]
fn a_record_shows_its_typed_notes() {
    let (_dir, store) = imported();
    let store = Arc::new(store);
    let index = Arc::new(TantivyIndex::in_memory(Parallelism::Serial).unwrap());
    index_pending(&*store, &*store, &*index, &options()).unwrap();
    let app = app(&store, &index);
    // Only the index note (555) of the record says this.
    let instance = index
        .search(
            &SearchQuery::words("nao publica indice"),
            SearchScope::Staff,
            0,
            1,
        )
        .unwrap()
        .hits[0]
        .instance;
    let (status, record) = get(&app, &format!("/record/{}", instance.0));
    assert_eq!(status, StatusCode::OK);
    assert!(record.contains("<dd>Não publica índice</dd>"));
}

#[test]
fn a_missing_record_is_404() {
    let (_dir, store) = imported();
    let store = Arc::new(store);
    let index = Arc::new(TantivyIndex::in_memory(Parallelism::Serial).unwrap());
    let (status, _) = get(&app(&store, &index), "/record/999999");
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[test]
fn an_instance_hidden_from_the_opac_is_found_only_by_the_staff() {
    let (_dir, store) = imported();
    let store = Arc::new(store);
    let index = Arc::new(TantivyIndex::in_memory(Parallelism::Serial).unwrap());
    index_pending(&*store, &*store, &*index, &options()).unwrap();
    let app = app(&store, &index);
    let instance = index
        .search(
            &SearchQuery::words("anuario estatistico"),
            SearchScope::Staff,
            0,
            1,
        )
        .unwrap()
        .hits[0]
        .instance;
    let version = store.instance(instance).unwrap().unwrap().version;
    store
        .submit(WriteCommand::Catalog(CatalogCommand::UpdateInstance(
            Box::new(InstanceUpdate {
                instance,
                row_version: version,
                changes: vec![InstanceChange::OpacHidden(true)],
            }),
        )))
        .unwrap()
        .wait()
        .unwrap();
    index_pending(&*store, &*store, &*index, &options()).unwrap();

    let (status, results) = get(&app, "/search?q=anuario+estatistico");
    assert_eq!(status, StatusCode::OK);
    assert!(!results.contains("Perfil - Anuário Estatístico"));
    let (status, _) = get(&app, &format!("/record/{}", instance.0));
    assert_eq!(status, StatusCode::NOT_FOUND);
    let public = index
        .search(
            &SearchQuery::words("anuario estatistico"),
            SearchScope::Public,
            0,
            10,
        )
        .unwrap();
    assert!(public.hits.iter().all(|hit| hit.instance != instance));
    let staff = index
        .search(
            &SearchQuery::words("anuario estatistico"),
            SearchScope::Staff,
            0,
            10,
        )
        .unwrap();
    assert!(staff.hits.iter().any(|hit| hit.instance == instance));

    // The export carries the mark at the edge, as Koha keeps it.
    let bytes = export_record(&*store, instance, RecordFormat::Marc21, &Limits::default())
        .unwrap()
        .unwrap();
    let mut reader = MarcReader::new(bytes.as_slice(), Limits::default());
    let record = reader.next_record().unwrap().unwrap();
    let local = record
        .fields()
        .find(|field| field.tag().to_string() == "942")
        .unwrap();
    assert!(String::from_utf8_lossy(local.raw()).contains("\x1fn1"));
}

/// docs/specs/catalogo/autoridades.md: a renamed agent shows in the OPAC, the search and
/// the exported MARC of every record that names it.
#[test]
fn a_renamed_agent_changes_every_record_that_names_it() {
    use elzevir_app::authorities::AuthorityReader;
    use elzevir_app::catalog::{AgentChange, AgentUpdate, CatalogRefusal};
    use elzevir_app::commands::WriteRefusal;
    use elzevir_domain::catalog::AgentId;
    use elzevir_store::CommandFailed;

    let (_dir, store) = imported();
    let store = Arc::new(store);
    let index = Arc::new(TantivyIndex::in_memory(Parallelism::Serial).unwrap());
    index_pending(&*store, &*store, &*index, &options()).unwrap();
    let app = app(&store, &index);
    let instance = index
        .search(
            &SearchQuery::words("anuario estatistico"),
            SearchScope::Staff,
            0,
            1,
        )
        .unwrap()
        .hits[0]
        .instance;
    let agent: i64 = store
        .readers()
        .read(|c| {
            c.query_row(
                "SELECT c.agent_id FROM contributions c JOIN instances i ON i.work_id = c.work_id
                 WHERE i.id = ?1 AND c.is_primary = 1",
                [instance.0],
                |row| row.get(0),
            )
        })
        .unwrap();
    let view = store.agent(AgentId(agent)).unwrap().unwrap();
    assert_eq!(view.heading, "PREFEITURA MUNICIPAL DE FOZ DO IGUACU");
    assert!(view.instances >= 1);
    let before = store.instance(instance).unwrap().unwrap().version;
    let rename = |row_version, heading: &str| {
        store
            .submit(WriteCommand::Catalog(CatalogCommand::UpdateAgent(
                Box::new(AgentUpdate {
                    agent: AgentId(agent),
                    row_version,
                    changes: vec![AgentChange::Heading(heading.to_owned())],
                }),
            )))
            .unwrap()
            .wait()
    };
    rename(1, "Foz do Iguaçu (PR). Prefeitura Municipal").unwrap();
    assert_eq!(store.agent(AgentId(agent)).unwrap().unwrap().version, 2);
    assert!(store.instance(instance).unwrap().unwrap().version > before);
    // An edit read before the rename is refused, and so is the heading of another agent.
    assert!(matches!(
        rename(1, "Outro"),
        Err(CommandFailed::Rejected(WriteRefusal::Catalog(
            CatalogRefusal::Stale { .. }
        )))
    ));
    let other: String = store
        .readers()
        .read(|c| {
            c.query_row(
                "SELECT heading FROM agents WHERE kind = 2 AND id <> ?1 LIMIT 1",
                [agent],
                |row| row.get(0),
            )
        })
        .unwrap();
    assert!(matches!(
        rename(2, &other),
        Err(CommandFailed::Rejected(WriteRefusal::Catalog(
            CatalogRefusal::AgentHeadingTaken(_)
        )))
    ));
    index_pending(&*store, &*store, &*index, &options()).unwrap();

    let (_, record) = get(&app, &format!("/record/{}", instance.0));
    assert!(record.contains("Foz do Iguaçu (PR). Prefeitura Municipal"));
    assert!(!record.contains("PREFEITURA MUNICIPAL DE FOZ DO IGUACU"));
    let found = index
        .search(
            &SearchQuery::words("foz iguacu prefeitura"),
            SearchScope::Staff,
            0,
            50,
        )
        .unwrap();
    assert!(found.hits.iter().any(|hit| hit.instance == instance
        && hit.main_contributor.as_deref() == Some("Foz do Iguaçu (PR). Prefeitura Municipal")));
    let bytes = export_record(&*store, instance, RecordFormat::Marc21, &Limits::default())
        .unwrap()
        .unwrap();
    let mut reader = MarcReader::new(bytes.as_slice(), Limits::default());
    let exported = reader.next_record().unwrap().unwrap();
    let main = exported
        .fields()
        .find(|field| field.tag().to_string() == "110")
        .unwrap();
    assert!(
        String::from_utf8_lossy(main.raw()).contains("Foz do Iguaçu (PR). Prefeitura Municipal")
    );
}

/// The Instance of the statistical yearbook and its Work.
fn yearbook(
    store: &ElzevirStore,
    index: &TantivyIndex,
) -> (elzevir_domain::catalog::InstanceId, i64) {
    let instance = index
        .search(
            &SearchQuery::words("anuario estatistico"),
            SearchScope::Staff,
            0,
            1,
        )
        .unwrap()
        .hits
        .first()
        .unwrap()
        .instance;
    let work = store
        .readers()
        .read(|c| {
            c.query_row(
                "SELECT work_id FROM instances WHERE id = ?1",
                [instance.0],
                |row| row.get(0),
            )
        })
        .unwrap();
    (instance, work)
}

/// Links `role` to `work` through the writer.
fn link(
    store: &ElzevirStore,
    work: i64,
    role: elzevir_app::catalog::AuthorityRole,
) -> Result<
    elzevir_app::commands::WriteOutput,
    elzevir_store::CommandFailed<elzevir_app::commands::WriteRefusal>,
> {
    store
        .submit(WriteCommand::Catalog(CatalogCommand::LinkAuthority(
            Box::new(elzevir_app::catalog::WorkAuthority {
                work: elzevir_domain::catalog::WorkId(work),
                role,
            }),
        )))
        .unwrap()
        .wait()
}

#[test]
fn a_linked_authority_joins_the_work_in_its_role() {
    use elzevir_app::catalog::{AuthorityRole, CatalogRefusal};
    use elzevir_app::commands::WriteRefusal;
    use elzevir_domain::catalog::{AgentKind, SubjectKind};
    use elzevir_store::CommandFailed;

    let (_dir, store) = imported();
    let store = Arc::new(store);
    let index = Arc::new(TantivyIndex::in_memory(Parallelism::Serial).unwrap());
    index_pending(&*store, &*store, &*index, &options()).unwrap();
    let app = app(&store, &index);
    let (instance, work) = yearbook(&store, &index);
    let before = store.instance(instance).unwrap().unwrap().version;
    let link = |work, role| link(&store, work, role);
    let illustrator = || AuthorityRole::Contribution {
        kind: AgentKind::Person,
        heading: "Portinari, Candido".to_owned(),
        relator: Some("ill".to_owned()),
    };
    link(work, illustrator()).unwrap();
    link(
        work,
        AuthorityRole::Subject {
            kind: SubjectKind::Topic,
            heading: "Estatistica municipal".to_owned(),
            thesaurus: None,
        },
    )
    .unwrap();
    assert!(store.instance(instance).unwrap().unwrap().version > before);
    // The new contributor comes last and is not the main one.
    let (last, primary): (String, bool) = store
        .readers()
        .read(|c| {
            c.query_row(
                "SELECT a.heading, c.is_primary FROM contributions c
                 JOIN agents a ON a.id = c.agent_id
                 WHERE c.work_id = ?1 ORDER BY c.position DESC LIMIT 1",
                [work],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
        })
        .unwrap();
    assert_eq!((last.as_str(), primary), ("Portinari, Candido", false));
    assert!(matches!(
        link(work, illustrator()),
        Err(CommandFailed::Rejected(WriteRefusal::Catalog(
            CatalogRefusal::AlreadyLinked(_)
        )))
    ));
    assert!(matches!(
        link(999_999, illustrator()),
        Err(CommandFailed::Rejected(WriteRefusal::Catalog(
            CatalogRefusal::UnknownWork(_)
        )))
    ));
    index_pending(&*store, &*store, &*index, &options()).unwrap();

    let (_, record) = get(&app, &format!("/record/{}", instance.0));
    assert!(record.contains("Portinari, Candido"));
    assert!(record.contains("Estatistica municipal"));
    let bytes = export_record(&*store, instance, RecordFormat::Marc21, &Limits::default())
        .unwrap()
        .unwrap();
    let mut reader = MarcReader::new(bytes.as_slice(), Limits::default());
    let exported = reader.next_record().unwrap().unwrap();
    let field = |tag: &str, text: &str| {
        exported.fields().any(|field| {
            field.tag().to_string() == tag && String::from_utf8_lossy(field.raw()).contains(text)
        })
    };
    assert!(field("700", "Portinari, Candido"));
    assert!(field("650", "Estatistica municipal"));
}

#[test]
fn a_place_of_publication_goes_to_one_instance_of_the_work() {
    use elzevir_app::catalog::{AuthorityRole, CatalogRefusal};
    use elzevir_app::commands::WriteRefusal;
    use elzevir_store::CommandFailed;

    let (_dir, store) = imported();
    let index = TantivyIndex::in_memory(Parallelism::Serial).unwrap();
    index_pending(&store, &store, &index, &options()).unwrap();
    let (instance, work) = yearbook(&store, &index);
    let link = |work, role| link(&store, work, role);

    let place = |instance| AuthorityRole::PublicationPlace {
        instance,
        heading: "Foz do Iguaçu, PR".to_owned(),
    };
    let before = store.instance(instance).unwrap().unwrap().version;
    link(work, place(instance)).unwrap();
    assert!(store.instance(instance).unwrap().unwrap().version > before);
    assert!(matches!(
        link(work, place(instance)),
        Err(CommandFailed::Rejected(WriteRefusal::Catalog(
            CatalogRefusal::AlreadyLinked(_)
        )))
    ));
    assert!(matches!(
        link(work + 1, place(instance)),
        Err(CommandFailed::Rejected(WriteRefusal::Catalog(
            CatalogRefusal::UnknownInstance(_)
        )))
    ));
    // The OPAC page does not show the place yet; the MARC export writes it.
    let bytes = export_record(&store, instance, RecordFormat::Marc21, &Limits::default())
        .unwrap()
        .unwrap();
    let mut reader = MarcReader::new(bytes.as_slice(), Limits::default());
    let exported = reader.next_record().unwrap().unwrap();
    assert!(exported.fields().any(|field| {
        ["260", "264"].contains(&field.tag().to_string().as_str())
            && String::from_utf8_lossy(field.raw()).contains("Foz do Iguaçu, PR")
    }));
}

#[test]
fn a_renamed_subject_changes_every_record_that_has_it() {
    use elzevir_app::authorities::AuthorityReader;
    use elzevir_app::catalog::{AgentChange, CatalogRefusal, SubjectUpdate};
    use elzevir_app::commands::WriteRefusal;
    use elzevir_domain::catalog::SubjectId;
    use elzevir_store::CommandFailed;

    let (_dir, store) = imported();
    let store = Arc::new(store);
    let index = Arc::new(TantivyIndex::in_memory(Parallelism::Serial).unwrap());
    index_pending(&*store, &*store, &*index, &options()).unwrap();
    let app = app(&store, &index);
    // A topical subject without typed subdivisions, and one with them.
    let (subject, instance, heading): (i64, i64, String) = store
        .readers()
        .read(|c| {
            c.query_row(
                "SELECT s.id, i.id, s.heading FROM subjects s
                 JOIN work_subjects w ON w.subject_id = s.id
                 JOIN instances i ON i.work_id = w.work_id
                 WHERE s.kind = 1 AND s.subdivisions = '' ORDER BY s.id, i.id LIMIT 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
        })
        .unwrap();
    let divided: i64 = store
        .readers()
        .read(|c| {
            c.query_row(
                "SELECT id FROM subjects WHERE subdivisions <> '' LIMIT 1",
                [],
                |row| row.get(0),
            )
        })
        .unwrap();
    let view = store.subject(SubjectId(subject)).unwrap().unwrap();
    assert_eq!(view.heading, heading);
    assert!(view.instances >= 1);
    let instance = elzevir_domain::catalog::InstanceId(instance);
    let before = store.instance(instance).unwrap().unwrap().version;
    let rename = |subject: i64, row_version, heading: &str| {
        store
            .submit(WriteCommand::Catalog(CatalogCommand::UpdateSubject(
                Box::new(SubjectUpdate {
                    subject: SubjectId(subject),
                    row_version,
                    changes: vec![AgentChange::Heading(heading.to_owned())],
                }),
            )))
            .unwrap()
            .wait()
    };
    rename(subject, 1, "Assunto renomeado no Elzevir").unwrap();
    assert_eq!(
        store.subject(SubjectId(subject)).unwrap().unwrap().version,
        2
    );
    assert!(store.instance(instance).unwrap().unwrap().version > before);
    assert!(matches!(
        rename(subject, 1, "Outro"),
        Err(CommandFailed::Rejected(WriteRefusal::Catalog(
            CatalogRefusal::Stale { .. }
        )))
    ));
    // A subject with typed subdivisions keeps as many parts after ` -- `.
    assert!(matches!(
        rename(divided, 1, "Sem subdivisoes"),
        Err(CommandFailed::Rejected(WriteRefusal::Catalog(
            CatalogRefusal::SubdivisionsChanged(_)
        )))
    ));
    index_pending(&*store, &*store, &*index, &options()).unwrap();

    let (_, record) = get(&app, &format!("/record/{}", instance.0));
    assert!(record.contains("Assunto renomeado no Elzevir"));
    let bytes = export_record(&*store, instance, RecordFormat::Marc21, &Limits::default())
        .unwrap()
        .unwrap();
    let mut reader = MarcReader::new(bytes.as_slice(), Limits::default());
    let exported = reader.next_record().unwrap().unwrap();
    assert!(exported.fields().any(|field| {
        field.tag().to_string() == "650"
            && String::from_utf8_lossy(field.raw()).contains("Assunto renomeado no Elzevir")
    }));
}

#[test]
fn an_authority_lists_its_identifiers_elsewhere_once() {
    use elzevir_app::authorities::AuthorityReader;
    use elzevir_domain::catalog::{AgentId, SubjectId};

    let (_dir, store) = imported();
    let (agent, value): (i64, String) = store
        .readers()
        .read(|c| {
            c.query_row(
                "SELECT c.agent_id, i.value FROM contribution_identifiers i
                 JOIN contributions c ON c.work_id = i.work_id AND c.position = i.contribution
                 ORDER BY c.agent_id LIMIT 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
        })
        .unwrap();
    let view = store.agent(AgentId(agent)).unwrap().unwrap();
    assert_eq!(
        view.identifiers
            .iter()
            .filter(|link| link.value == value)
            .count(),
        1
    );
    let (subject, value): (i64, String) = store
        .readers()
        .read(|c| {
            c.query_row(
                "SELECT w.subject_id, i.value FROM subject_identifiers i
                 JOIN work_subjects w ON w.work_id = i.work_id AND w.position = i.subject
                 ORDER BY w.subject_id LIMIT 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
        })
        .unwrap();
    let view = store.subject(SubjectId(subject)).unwrap().unwrap();
    assert!(view.identifiers.iter().any(|link| link.value == value));
}

#[test]
fn an_lcgft_genre_shows_its_label_on_the_record() {
    let (_dir, store) = imported();
    let store = Arc::new(store);
    let index = Arc::new(TantivyIndex::in_memory(Parallelism::Serial).unwrap());
    index_pending(&*store, &*store, &*index, &options()).unwrap();
    let app = app(&store, &index);
    // A novel: its 008/33 `f` becomes the LCGFT term by IRI, and the OPAC shows the
    // Library of Congress's label for it.
    let instance: i64 = store
        .readers()
        .read(|c| {
            c.query_row(
                "SELECT i.id FROM statements s JOIN instances i ON i.work_id = s.subject_id
                 WHERE s.subject_kind = 2 AND s.parent IS NULL AND s.iri = ?1
                 ORDER BY i.id LIMIT 1",
                ["http://id.loc.gov/authorities/genreForms/gf2015026020"],
                |row| row.get(0),
            )
        })
        .unwrap();
    let (status, record) = get(&app, &format!("/record/{instance}"));
    assert_eq!(status, StatusCode::OK);
    assert!(record.contains("Novels"));
    assert!(!record.contains("gf2015026020"));
}

// SPDX-License-Identifier: GPL-3.0-or-later
//! Preview and confirmation use the same preparation and catalog rules.
#![expect(
    clippy::unwrap_used,
    reason = "test database and fixture setup must succeed"
)]

use elzevir_app::catalog::CatalogCommand;
use elzevir_app::codes::{CodeTable, NamedCode};
use elzevir_app::commands::WriteCommand;
use elzevir_app::import::{ImportOptions, import_marc21};
use elzevir_app::matching::{ItemAction, Matching, OnNoMatch};
use elzevir_domain::catalog::MatchRule;
use elzevir_marc::{Field, Record, Subfield, Tag};
use elzevir_store::{ElzevirHandler, ElzevirStore, StoreConfig};

fn data(tag: [u8; 3], pairs: &[(u8, &str)]) -> Field {
    Field::Data {
        tag: Tag::new(tag),
        indicators: *b"  ",
        subfields: pairs
            .iter()
            .map(|&(code, value)| Subfield {
                code,
                value: value.as_bytes().to_vec(),
            })
            .collect(),
    }
}

fn source(barcode: Option<&str>) -> Vec<u8> {
    let mut item = vec![(b'a', "Synthetic branch"), (b'y', "Synthetic policy")];
    if let Some(barcode) = barcode {
        item.push((b'p', barcode));
    }
    let record = Record {
        leader: *b"00000nam a2200000 a 4500",
        fields: vec![
            data(*b"245", &[(b'a', "Synthetic title")]),
            data(*b"999", &[(b'c', "42")]),
            data(*b"952", &item),
        ],
    };
    let mut bytes = Vec::new();
    record.write_iso2709(&mut bytes).unwrap();
    bytes
}

fn open(dir: &tempfile::TempDir) -> ElzevirStore {
    ElzevirStore::open(
        &dir.path().join("preview.db"),
        &StoreConfig::default(),
        ElzevirHandler,
    )
    .unwrap()
}

fn seed(store: &ElzevirStore) {
    let codes = [
        (CodeTable::Branch, "Synthetic branch"),
        (CodeTable::ItemType, "Synthetic policy"),
    ]
    .into_iter()
    .map(|(table, name)| NamedCode {
        table,
        name: name.into(),
        code: name.into(),
    })
    .collect();
    store
        .submit(WriteCommand::Catalog(CatalogCommand::NameCodes(codes)))
        .unwrap()
        .wait()
        .unwrap();
}

#[test]
fn preview_refuses_the_same_unknown_registry_code_as_the_writer_and_keeps_the_digest() {
    let directory = tempfile::tempdir().unwrap();
    let store = open(&directory);
    let source = source(Some("SYNTHETIC-1"));
    let options = ImportOptions {
        origin: Some("test-koha".into()),
        ..ImportOptions::default()
    };
    let before = store.digest().unwrap();
    let forecast = store
        .readers()
        .preview_import(source.as_slice(), options.clone())
        .unwrap();
    assert_eq!(forecast.issues.len(), 1);
    let issue = forecast.issues.first().unwrap();
    assert_eq!((issue.index, issue.offset, issue.item), (0, 0, Some(0)));
    assert_eq!(issue.reason, "unknown branch");
    assert_eq!(store.digest().unwrap(), before);
    let confirmed = import_marc21(source.as_slice(), &store, &options).unwrap();
    assert_eq!(confirmed.imported, 0);
    assert!(
        confirmed
            .failures
            .first()
            .unwrap()
            .failure
            .to_string()
            .contains(&issue.reason)
    );
}

#[test]
fn preview_reports_existing_and_batch_conflicts_and_respects_item_replacement() {
    let directory = tempfile::tempdir().unwrap();
    let store = open(&directory);
    seed(&store);
    let source = source(Some("SYNTHETIC-2"));
    let options = ImportOptions {
        origin: Some("test-koha".into()),
        ..ImportOptions::default()
    };
    assert_eq!(
        import_marc21(source.as_slice(), &store, &options)
            .unwrap()
            .items,
        1
    );

    let before = store.digest().unwrap();
    let matching = Matching::new(MatchRule::KohaBiblionumber);
    let options = ImportOptions {
        matching: Some(matching),
        ..options
    };
    let repeated = source.repeat(2);
    let forecast = store
        .readers()
        .preview_import(repeated.as_slice(), options.clone())
        .unwrap();
    assert_eq!(forecast.matches.len(), 2);
    for reason in [
        "barcode already in use",
        "barcode already present in the batch",
        "possible match within the batch",
    ] {
        assert!(
            forecast
                .issues
                .iter()
                .any(|issue| issue.reason.starts_with(reason))
        );
    }
    let mut options = ImportOptions {
        matching: Some(Matching {
            items: ItemAction::Replace,
            ..matching
        }),
        ..options
    };
    assert!(
        store
            .readers()
            .preview_import(source.as_slice(), options.clone())
            .unwrap()
            .issues
            .is_empty()
    );
    options.reviewed = store
        .readers()
        .preview_import(source.as_slice(), options.clone())
        .unwrap()
        .scopes
        .into_iter()
        .map(|(_, scope)| scope)
        .collect();
    assert_eq!(store.digest().unwrap(), before);
    assert_eq!(
        import_marc21(source.as_slice(), &store, &options)
            .unwrap()
            .replaced_items,
        1
    );
}

#[test]
fn absence_of_barcode_requires_review_unless_the_item_would_be_skipped() {
    let directory = tempfile::tempdir().unwrap();
    let store = open(&directory);
    seed(&store);
    let source = source(None);
    let options = ImportOptions {
        origin: Some("test-koha".into()),
        ..ImportOptions::default()
    };
    let before = store.digest().unwrap();
    let forecast = store
        .readers()
        .preview_import(source.as_slice(), options.clone())
        .unwrap();
    assert_eq!(forecast.issues.first().unwrap().reason, "missing barcode");
    let options = ImportOptions {
        matching: Some(Matching {
            on_no_match: OnNoMatch::Ignore,
            ..Matching::new(MatchRule::KohaBiblionumber)
        }),
        ..options
    };
    assert!(
        store
            .readers()
            .preview_import(source.as_slice(), options)
            .unwrap()
            .issues
            .is_empty()
    );
    assert_eq!(store.digest().unwrap(), before);
}

fn binary(fields: Vec<Field>) -> Vec<u8> {
    let mut bytes = Vec::new();
    Record {
        leader: *b"00000nam a2200000 a 4500",
        fields,
    }
    .write_iso2709(&mut bytes)
    .unwrap();
    bytes
}

fn edition(title: &str, local: &str, barcode: Option<&str>) -> Vec<u8> {
    let mut fields = vec![
        data(*b"245", &[(b'a', title)]),
        data(*b"020", &[(b'a', "9788560342013")]),
        data(*b"999", &[(b'c', local)]),
    ];
    if let Some(barcode) = barcode {
        fields.push(data(
            *b"952",
            &[
                (b'a', "Synthetic branch"),
                (b'y', "Synthetic policy"),
                (b'p', barcode),
            ],
        ));
    }
    binary(fields)
}

#[test]
fn ambiguous_stale_and_cross_instance_matches_cannot_write_even_part_of_a_unit() {
    use elzevir_app::matching::OnMatch;
    let dir = tempfile::tempdir().unwrap();
    let store = open(&dir);
    seed(&store);
    let source = edition("Before", "42", Some("A"));
    let origin = ImportOptions {
        origin: Some("koha-a".into()),
        ..ImportOptions::default()
    };
    assert_eq!(
        import_marc21(source.as_slice(), &store, &origin)
            .unwrap()
            .imported,
        1
    );
    let overlay = edition("After", "42", None);
    let mut options = ImportOptions {
        matching: Some(Matching {
            on_match: OnMatch::Replace,
            items: ItemAction::Replace,
            ..Matching::new(MatchRule::Isbn)
        }),
        ..origin.clone()
    };
    let scope = store
        .readers()
        .preview_import(overlay.as_slice(), options.clone())
        .unwrap()
        .scopes
        .remove(0)
        .1;
    assert_eq!(scope.editions.len(), 1);
    assert_eq!(scope.holdings.first().unwrap().branch, "Synthetic branch");
    options.reviewed.push(scope);
    let other = edition("Other", "42", Some("B"));
    let distinct_origin = ImportOptions {
        origin: Some("koha-b".into()),
        matching: Some(Matching::new(MatchRule::KohaBiblionumber)),
        ..ImportOptions::default()
    };
    assert_eq!(
        import_marc21(other.as_slice(), &store, &distinct_origin)
            .unwrap()
            .imported,
        1
    );
    let before = store.digest().unwrap();
    let refused = |bytes: &[u8], opts: &ImportOptions, reason: &str| {
        let result = import_marc21(bytes, &store, opts).unwrap();
        assert_eq!(result.failures.len(), 1);
        assert!(
            result
                .failures
                .first()
                .unwrap()
                .failure
                .to_string()
                .contains(reason)
        );
        assert_eq!(store.digest().unwrap(), before);
    };
    refused(&overlay, &options, "changed after preview");
    options.reviewed.clear();
    refused(&overlay, &options, "multiple candidate");
    let scope = store
        .readers()
        .preview_import(overlay.as_slice(), options.clone())
        .unwrap();
    assert_eq!(scope.scopes.first().unwrap().1.candidates.len(), 2);
    options.matching.as_mut().unwrap().rule = MatchRule::KohaBiblionumber;
    refused(
        &edition("Wrong", "42", Some("B")),
        &options,
        "another Instance",
    );
    options.origin = None;
    refused(&overlay, &options, "provenance");
}

#[test]
fn preview_tracks_shared_work_versions_holding_branch_and_open_loan() {
    use elzevir_app::matching::OnMatch;
    let dir = tempfile::tempdir().unwrap();
    let store = open(&dir);
    seed(&store);
    let src = edition("Target", "10", Some("001"));
    let options = ImportOptions {
        origin: Some("installation".into()),
        ..ImportOptions::default()
    };
    assert_eq!(
        import_marc21(src.as_slice(), &store, &options)
            .unwrap()
            .imported,
        1
    );
    let connection = rusqlite::Connection::open(dir.path().join("preview.db")).unwrap();
    connection.execute_batch("INSERT INTO instances(work_id,title) VALUES(1,'Sibling');
       INSERT INTO patron_categories(name,enrolment_months) VALUES('Standard',12);
       INSERT INTO patrons(cardnumber,surname,branch_id,category_id,enrolled,expires) VALUES('1','Reader',1,1,'2026-01-01','2027-01-01');
       INSERT INTO loans(item_id,patron_id,branch_id,issued_at,due,on_site) VALUES(1,1,1,'2026-01-01','2026-02-01',0);").unwrap();
    let mut options = ImportOptions {
        matching: Some(Matching {
            on_match: OnMatch::Replace,
            items: ItemAction::Ignore,
            ..Matching::new(MatchRule::KohaBiblionumber)
        }),
        ..options
    };
    let scope = store
        .readers()
        .preview_import(src.as_slice(), options.clone())
        .unwrap()
        .scopes
        .remove(0)
        .1;
    assert_eq!(scope.editions.len(), 2);
    assert_eq!(
        scope.holdings.first().unwrap().loan,
        Some((1, "2026-02-01".into()))
    );
    options.reviewed.push(scope);
    connection
        .execute(
            "UPDATE instances SET row_version=row_version+1 WHERE id=2",
            [],
        )
        .unwrap();
    let before = store.digest().unwrap();
    assert_eq!(
        import_marc21(src.as_slice(), &store, &options)
            .unwrap()
            .failures
            .len(),
        1
    );
    assert_eq!(store.digest().unwrap(), before);
}

#[test]
fn two_copies_retain_separate_barcodes_inventory_numbers_and_chronology() {
    use elzevir_app::export::stored_record;
    use elzevir_domain::catalog::InstanceId;
    let dir = tempfile::tempdir().unwrap();
    let store = open(&dir);
    seed(&store);
    let mut fields = vec![
        data(*b"245", &[(b'a', "Serial")]),
        data(
            *b"863",
            &[
                (b'8', "1.1"),
                (b'a', "v.12"),
                (b'b', "n.3"),
                (b'i', "2020"),
                (b'j', "03"),
            ],
        ),
    ];
    for (barcode, tombo, copy) in [("00001", "0100", "1"), ("00002", "0101", "2")] {
        fields.push(data(*b"876", &[(b'8', "1.1"), (b'p', barcode)]));
        fields.push(data(
            *b"952",
            &[
                (b'a', "Synthetic branch"),
                (b'y', "Synthetic policy"),
                (b'p', barcode),
                (b'h', "v.12:3 (março 2020)"),
                (b'i', tombo),
                (b't', copy),
            ],
        ));
    }
    let src = binary(fields);
    assert_eq!(
        import_marc21(src.as_slice(), &store, &ImportOptions::default())
            .unwrap()
            .items,
        2
    );
    let stored = stored_record(&store, InstanceId(1)).unwrap().unwrap();
    let copies = stored.record.items.marc();
    for (item, (barcode, tombo, copy)) in copies
        .iter()
        .zip([("00001", "0100", "1"), ("00002", "0101", "2")])
    {
        assert_eq!(item.barcode.as_deref(), Some(barcode));
        assert_eq!(item.inventory_number.as_deref(), Some(tombo));
        assert_eq!(item.copy_number.as_deref(), Some(copy));
        assert_eq!(item.issue_statement.as_deref(), Some("v.12:3 (março 2020)"));
        assert_eq!(item.enumeration.as_deref(), Some("v.12 n.3"));
        assert_eq!(item.chronology.as_deref(), Some("2020 03"));
    }
}

#[test]
fn per_record_encoding_and_homonymous_headings_survive_the_writer() {
    use elzevir_app::export::stored_record;
    use elzevir_domain::catalog::InstanceId;
    let dir = tempfile::tempdir().unwrap();
    let store = open(&dir);
    let options = ImportOptions {
        origin: Some("legacy".into()),
        ..ImportOptions::default()
    };
    let mut all = Vec::new();
    for (i, code) in ["50  ", "0103"].iter().enumerate() {
        let id = if i == 0 { "ext-1" } else { "ext-2" };
        let mut title = data(*b"200", &[(b'a', "Café")]);
        if i == 1
            && let Field::Data { subfields, .. } = &mut title
        {
            subfields.first_mut().unwrap().value = b"Caf\xc2e".to_vec();
        }
        let mark = format!("{}{code}", " ".repeat(26));
        let raw = binary(vec![
            data(*b"100", &[(b'a', &mark)]),
            title,
            data(*b"700", &[(b'a', "Silva"), (b'b', "Ana"), (b'3', id)]),
            data(*b"606", &[(b'a', "Homonym"), (b'1', id)]),
        ]);
        all.extend_from_slice(&raw);
        assert_eq!(
            import_marc21(raw.as_slice(), &store, &options)
                .unwrap()
                .imported,
            1
        );
        let stored = stored_record(&store, InstanceId(i64::try_from(i + 1).unwrap()))
            .unwrap()
            .unwrap()
            .record;
        let original = stored.instance.original.unwrap();
        assert_eq!(original.bytes, raw);
        assert_eq!(original.declared, *code);
        assert_eq!(original.family, "UNIMARC");
        assert_eq!(stored.instance.title, "Café");
        assert_eq!(
            stored
                .contributions
                .first()
                .unwrap()
                .identifiers
                .first()
                .unwrap()
                .value,
            id
        );
        assert_eq!(
            stored
                .subjects
                .first()
                .unwrap()
                .identifiers
                .first()
                .unwrap()
                .value,
            id
        );
    }
    for table in ["agents", "subjects"] {
        let n = store
            .readers()
            .read(|c| {
                c.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| {
                    r.get::<_, i64>(0)
                })
            })
            .unwrap();
        assert_eq!(n, 2);
    }
    let report = store
        .readers()
        .preview_import(all.as_slice(), options)
        .unwrap();
    assert_eq!(report.prepared, 2);
}

#[test]
fn koha_fixture_keeps_its_ten_qualified_records_and_eighteen_external_items() {
    use elzevir_app::import::import_marc_text;
    let dir = tempfile::tempdir().unwrap();
    let store = open(&dir);
    let bytes = include_bytes!("../../../samples-import-tool/koha-marc21-sample.mrk");
    let forecast = store
        .readers()
        .preview_import(bytes.as_slice(), ImportOptions::default())
        .unwrap();
    let mut codes = std::collections::BTreeSet::new();
    for raw in
        elzevir_marc::MarcTextReader::new(bytes.as_slice(), elzevir_marc::MrkLimits::default())
    {
        let mapped = elzevir_app::marc_mapping::marc_to_bibframe(&raw.unwrap(), None).unwrap();
        for item in mapped.items.marc() {
            codes.insert((true, item.home_branch.clone()));
            codes.insert((true, item.holding_branch.clone()));
            codes.insert((false, item.item_type.clone()));
        }
    }
    let names = codes
        .into_iter()
        .map(|(branch, name)| NamedCode {
            table: if branch {
                CodeTable::Branch
            } else {
                CodeTable::ItemType
            },
            code: name.clone(),
            name,
        })
        .collect();
    store
        .submit(WriteCommand::Catalog(CatalogCommand::NameCodes(names)))
        .unwrap()
        .wait()
        .unwrap();
    assert_eq!(forecast.read, 10);
    let options = ImportOptions {
        origin: Some("fixture".into()),
        matching: Some(Matching::new(MatchRule::KohaBiblionumber)),
        ..ImportOptions::default()
    };
    let loaded = import_marc_text(bytes.as_slice(), &store, &options).unwrap();
    assert_eq!((loaded.imported, loaded.items), (10, 18));
    let counts=store.readers().read(|c|c.query_row("SELECT (SELECT count(*) FROM import_identities),(SELECT count(*) FROM item_origins)",[],|r|Ok((r.get::<_,i64>(0)?,r.get::<_,i64>(1)?)))).unwrap();
    assert_eq!(counts, (20, 18));
}

#[test]
fn marc8_conversion_retains_the_original_record_and_its_declaration() {
    use elzevir_app::export::stored_record;
    let dir = tempfile::tempdir().unwrap();
    let store = open(&dir);
    let mut title = data(*b"245", &[(b'a', "Title")]);
    if let Field::Data { subfields, .. } = &mut title {
        subfields.first_mut().unwrap().value = b"Caf\xe2e".to_vec();
    }
    let mut raw = binary(vec![title]);
    raw[9] = b' ';
    assert_eq!(
        import_marc21(raw.as_slice(), &store, &ImportOptions::default())
            .unwrap()
            .imported,
        1
    );
    let stored = stored_record(&store, elzevir_domain::catalog::InstanceId(1))
        .unwrap()
        .unwrap()
        .record;
    let original = stored.instance.original.unwrap();
    assert_eq!(
        (original.declared.as_str(), original.decoded.as_str()),
        ("MARC-8", "Marc8")
    );
    assert_eq!(original.bytes, raw);
    assert_eq!(stored.instance.title, "Cafe\u{301}");
}

#[test]
fn unqualified_homonyms_do_not_create_a_shared_authority() {
    let dir = tempfile::tempdir().unwrap();
    let store = open(&dir);
    let bytes = binary(vec![
        data(*b"245", &[(b'a', "Test")]),
        data(*b"100", &[(b'a', "Same name")]),
        data(*b"650", &[(b'a', "Same label")]),
    ]);
    let report = import_marc21(
        bytes.repeat(2).as_slice(),
        &store,
        &ImportOptions::default(),
    )
    .unwrap();
    assert_eq!(report.imported, 2);
    let counts: (i64, i64) = store
        .readers()
        .read(|c| {
            c.query_row(
                "SELECT (SELECT COUNT(DISTINCT agent_id) FROM contributions),
                (SELECT COUNT(DISTINCT subject_id) FROM work_subjects)",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
        })
        .unwrap();
    assert_eq!(counts, (2, 2));
}

#[test]
fn confirmed_overlay_preserves_local_fields_and_omitted_item_columns() {
    use elzevir_app::{export::stored_record, matching::OnMatch};
    use elzevir_domain::catalog::InstanceId;
    let dir = tempfile::tempdir().unwrap();
    let store = open(&dir);
    seed(&store);
    let mut fields = vec![
        data(*b"245", &[(b'a', "Original")]),
        data(*b"999", &[(b'c', "123")]),
        data(*b"590", &[(b'a', "Staff annotation")]),
        data(*b"991", &[(b'x', "Accession history")]),
        data(
            *b"952",
            &[
                (b'a', "Synthetic branch"),
                (b'y', "Synthetic policy"),
                (b'p', "001"),
                (b'i', "0007"),
                (b't', "2"),
                (b'o', "LOCAL"),
                (b'x', "Internal repair note"),
                (b'4', "1"),
            ],
        ),
    ];
    let bytes = binary(fields.clone());
    let mut options = ImportOptions {
        origin: Some("catalog-a".into()),
        ..ImportOptions::default()
    };
    assert_eq!(
        import_marc21(bytes.as_slice(), &store, &options)
            .unwrap()
            .items,
        1
    );
    fields.retain(|f| matches!(f.tag().as_bytes(), b"999"));
    fields.extend([
        data(*b"245", &[(b'a', "Revised")]),
        data(*b"090", &[(b'a', "FALLBACK")]),
        data(
            *b"952",
            &[
                (b'a', "Synthetic branch"),
                (b'y', "Synthetic policy"),
                (b'p', "001"),
            ],
        ),
    ]);
    let overlay = binary(fields);
    options.matching = Some(Matching {
        on_match: OnMatch::Replace,
        items: ItemAction::Replace,
        ..Matching::new(MatchRule::KohaBiblionumber)
    });
    let report = store
        .readers()
        .preview_import(overlay.as_slice(), options.clone())
        .unwrap();
    assert!(report.issues.is_empty());
    options.reviewed = report.scopes.into_iter().map(|(_, s)| s).collect();
    let applied = import_marc21(overlay.as_slice(), &store, &options).unwrap();
    assert!(applied.failures.is_empty());
    let stored = stored_record(&store, InstanceId(1))
        .unwrap()
        .unwrap()
        .record;
    assert_eq!(stored.instance.title, "Revised");
    assert!(
        stored
            .instance
            .notes
            .iter()
            .any(|n| n.text == "Staff annotation" && !n.is_public())
    );
    let item = stored.items.marc().first().unwrap();
    assert_eq!(item.inventory_number.as_deref(), Some("0007"));
    assert_eq!(item.call_number.as_deref(), Some("LOCAL"));
    assert_eq!(item.copy_number.as_deref(), Some("2"));
    assert_eq!(item.status.damaged, 1);
    let raw = stored.source.unwrap();
    let record = elzevir_marc::RecordRef::parse(&raw, &elzevir_marc::Limits::default()).unwrap();
    assert_eq!(
        record
            .fields_with_tag(Tag::new(*b"952"))
            .next()
            .unwrap()
            .subfield_values(b'x')
            .next(),
        Some(b"Internal repair note".as_slice())
    );
    assert_eq!(
        record
            .fields_with_tag(Tag::new(*b"991"))
            .next()
            .unwrap()
            .subfield_values(b'x')
            .next(),
        Some(b"Accession history".as_slice())
    );
}

#[test]
fn textual_formats_keep_source_bytes_before_parsing_or_normalization() {
    use elzevir_app::{
        export::stored_record,
        import::{import_marc_text, import_marcxml},
    };
    use elzevir_domain::catalog::InstanceId;
    let dir = tempfile::tempdir().unwrap();
    let store = open(&dir);
    let record = "<m:record ><m:leader>00000nam a2200000 i 4500</m:leader><!--original--><m:datafield ind2='0' tag='245' ind1='1'><m:subfield code='a'>A &amp; B</m:subfield></m:datafield></m:record>";
    let xml = format!(
        "\u{feff}<?xml version='1.0'?><m:collection xmlns:m='http://www.loc.gov/MARC21/slim'>  {record}\n{record}</m:collection>"
    );
    let options = ImportOptions::default();
    assert_eq!(
        import_marcxml(xml.as_bytes(), &store, &options)
            .unwrap()
            .imported,
        2
    );
    for id in [1, 2] {
        let original = stored_record(&store, InstanceId(id))
            .unwrap()
            .unwrap()
            .record
            .instance
            .original
            .unwrap();
        assert_eq!(original.serialization, "MARCXML");
        assert_eq!(original.declared, "undeclared");
        assert_eq!(original.decoded, "UTF-8");
        assert_eq!(original.bytes, record.as_bytes());
    }
    let mrk = b"=LDR  00000nam a2200000 i 4500\r\n=245  10$aText\r\n";
    assert_eq!(
        import_marc_text(mrk.as_slice(), &store, &options)
            .unwrap()
            .imported,
        1
    );
    let original = stored_record(&store, InstanceId(3))
        .unwrap()
        .unwrap()
        .record
        .instance
        .original
        .unwrap();
    assert_eq!(original.serialization, "MRK");
    assert_eq!(original.bytes, mrk);
}

#[test]
fn mnemonic_fixtures_keep_local_fields_and_agency_control_numbers() {
    use elzevir_app::import::import_marc_text;
    let dir = tempfile::tempdir().unwrap();
    let store = open(&dir);
    let samples: [(&str, &[u8], usize); 2] = [
        (
            "biblivre",
            include_bytes!(
                "../../../samples-import-tool/biblivre_simulacao_10_registros_com_exemplares_sinteticos.mrk"
            ),
            18,
        ),
        (
            "pergamum",
            include_bytes!("../../../samples-import-tool/pergamum_simulacao_10_registros.mrk"),
            0,
        ),
    ];
    for (origin, bytes, local_fields) in samples {
        let options = ImportOptions {
            origin: Some(origin.into()),
            ..ImportOptions::default()
        };
        let r = import_marc_text(bytes, &store, &options).unwrap();
        assert_eq!((r.imported, r.items), (10, 0));
        let actual = store
            .readers()
            .read(|c| {
                let mut query =
                    c.prepare("SELECT original FROM original_records WHERE origin=?1")?;
                let mut count = 0;
                for raw in query.query_map([origin], |r| r.get::<_, Vec<u8>>(0))? {
                    count += raw?
                        .split(|b| *b == b'\n')
                        .filter(|l| l.starts_with(b"=949"))
                        .count();
                }
                Ok::<_, rusqlite::Error>(count)
            })
            .unwrap();
        assert_eq!(actual, local_fields);
        let numbers = store
            .readers()
            .read(|c| {
                c.query_row(
                    "SELECT count(*) FROM import_identities
            WHERE origin=?1 AND system LIKE 'marc21.001%'",
                    [origin],
                    |r| r.get::<_, i64>(0),
                )
            })
            .unwrap();
        assert_eq!(numbers, 10);
    }
}

#[test]
fn refusal_on_the_last_item_rolls_back_a_reviewed_replacement() {
    use elzevir_app::matching::OnMatch;
    let dir = tempfile::tempdir().unwrap();
    let store = open(&dir);
    seed(&store);
    let mut options = ImportOptions {
        origin: Some("A".into()),
        ..ImportOptions::default()
    };
    assert_eq!(
        import_marc21(
            edition("Before", "51", Some("0001")).as_slice(),
            &store,
            &options
        )
        .unwrap()
        .imported,
        1
    );
    options.matching = Some(Matching {
        on_match: OnMatch::Replace,
        items: ItemAction::Replace,
        ..Matching::new(MatchRule::KohaBiblionumber)
    });
    let overlay = binary(vec![
        data(*b"245", &[(b'a', "Revised")]),
        data(*b"999", &[(b'c', "51")]),
        data(
            *b"952",
            &[
                (b'a', "Synthetic branch"),
                (b'y', "Synthetic policy"),
                (b'p', "0001"),
                (b'i', "changed"),
            ],
        ),
        data(
            *b"952",
            &[
                (b'a', "Synthetic branch"),
                (b'y', "Retired"),
                (b'p', "0002"),
            ],
        ),
    ]);
    // The policy exists at preview time and is withdrawn before the writer starts.
    let c = rusqlite::Connection::open(dir.path().join("preview.db")).unwrap();
    c.execute_batch(
        "INSERT INTO circulation_policies(id,name) VALUES(2,'Retired');
       INSERT INTO koha_codes(kind,code,local_id) VALUES(4,'Retired',2);",
    )
    .unwrap();
    let r = store
        .readers()
        .preview_import(overlay.as_slice(), options.clone())
        .unwrap();
    assert!(r.issues.is_empty());
    options.reviewed = r.scopes.into_iter().map(|(_, s)| s).collect();
    c.execute_batch("DELETE FROM koha_codes WHERE kind=4 AND local_id=2; DELETE FROM circulation_policies WHERE id=2;").unwrap();
    let before = store.digest().unwrap();
    assert_eq!(
        import_marc21(overlay.as_slice(), &store, &options)
            .unwrap()
            .failures
            .len(),
        1
    );
    assert_eq!(store.digest().unwrap(), before);
}

#[test]
fn mixed_stream_detects_format_for_each_bibliographic_record_and_refuses_authorities() {
    use elzevir_app::export::stored_record;
    use elzevir_domain::catalog::InstanceId;
    let dir = tempfile::tempdir().unwrap();
    let store = open(&dir);
    let marc = edition("MARC 21", "1", None);
    let legacy = binary(vec![
        data(*b"200", &[(b'a', "UNIMARC")]),
        data(
            *b"607",
            &[
                (b'a', "Lisboa"),
                (b'0', "(PT)10"),
                (b'1', "urn:place:10"),
                (b'2', "local"),
            ],
        ),
        data(*b"316", &[(b'a', "Team only")]),
    ]);
    let mut authority = legacy.clone();
    authority[6] = b'x';
    let mut unsupported = binary(vec![
        data(*b"100", &[(b'a', &format!("{}0399", " ".repeat(26)))]),
        data(*b"200", &[(b'a', "Undecodable")]),
    ]);
    let mut input = marc;
    input.extend_from_slice(&authority);
    input.append(&mut unsupported);
    input.extend_from_slice(&legacy);
    let r = import_marc21(input.as_slice(), &store, &ImportOptions::default()).unwrap();
    assert_eq!((r.read, r.imported, r.failures.len()), (4, 2, 2));
    let view = stored_record(&store, InstanceId(2))
        .unwrap()
        .unwrap()
        .record;
    assert_eq!(view.instance.original.unwrap().bytes, legacy);
    let heading = view.subjects.first().unwrap();
    assert_eq!(heading.identifiers.len(), 2);
    assert_eq!(heading.thesaurus.as_deref(), Some("local"));
    assert!(
        view.instance
            .notes
            .iter()
            .any(|n| n.text == "Team only" && !n.is_public())
    );
}

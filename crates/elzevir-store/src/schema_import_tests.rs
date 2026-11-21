// SPDX-License-Identifier: GPL-3.0-or-later
use super::*;

#[test]
fn migration_61_preserves_legacy_links_and_requeues_private_notes() {
    let mut c = Connection::open_in_memory().unwrap();
    migrate_range(&mut c, 0, 60, true).unwrap();
    c.execute_batch(
        "INSERT INTO works(id,title) VALUES(4,'Work');
       INSERT INTO instances(id,work_id,title) VALUES(8,4,'Edition');
       INSERT INTO agents(id,kind,heading) VALUES(13,1,'Name');
       INSERT INTO contributions(work_id,position,agent_id) VALUES(4,0,13);
       INSERT INTO subjects(id,kind,heading) VALUES(21,1,'Topic');
       INSERT INTO work_subjects(work_id,position,subject_id) VALUES(4,0,21);
       INSERT INTO instance_notes(instance_id,position,kind,text) VALUES(8,0,3,'Local');
       INSERT INTO audit_log(kind,command) VALUES('catalog.add_record','{}');",
    )
    .unwrap();
    migrate(&mut c).unwrap();
    let links: (i64, i64) = c
        .query_row(
            "SELECT a.id,s.id FROM contributions co JOIN agents a ON co.agent_id=a.id
        JOIN work_subjects ws ON co.work_id=ws.work_id JOIN subjects s ON ws.subject_id=s.id",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(links, (13, 21));
    let effect: i64 = c
        .query_row(
            "SELECT count(*) FROM effect_outbox WHERE kind='catalog.reindex_instance'
        AND json_extract(payload,'$.instance')=8",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(effect, 1);
    let known: i64 = c
        .query_row("SELECT count(*) FROM instance_origins", [], |r| r.get(0))
        .unwrap();
    assert_eq!(known, 0);
    assert!(
        !crate::export::notes(&c, 8)
            .unwrap()
            .first()
            .unwrap()
            .is_public()
    );
    assert!(
        c.prepare("PRAGMA foreign_key_check")
            .unwrap()
            .query([])
            .unwrap()
            .next()
            .unwrap()
            .is_none()
    );
}

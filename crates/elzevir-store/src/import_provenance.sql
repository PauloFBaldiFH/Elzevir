-- SPDX-License-Identifier: GPL-3.0-or-later
CREATE TABLE agents_new (
   id INTEGER PRIMARY KEY, kind INTEGER NOT NULL CHECK(kind BETWEEN 1 AND 255),
   heading TEXT NOT NULL, name_parts TEXT NOT NULL DEFAULT '', row_version INTEGER NOT NULL DEFAULT 1,
   identity TEXT NOT NULL DEFAULT '', UNIQUE(kind,heading,identity)
) STRICT;
INSERT INTO agents_new(id,kind,heading,name_parts,row_version) SELECT id,kind,heading,name_parts,row_version FROM agents;
DROP TABLE agents;
ALTER TABLE agents_new RENAME TO agents;
CREATE INDEX agents_by_heading ON agents(heading COLLATE NOCASE);
CREATE TABLE subjects_new (
   id INTEGER PRIMARY KEY,kind INTEGER NOT NULL CHECK(kind BETWEEN 1 AND 255),
   heading TEXT NOT NULL, thesaurus INTEGER REFERENCES terms(id),subdivisions TEXT NOT NULL DEFAULT '',
   name_parts TEXT NOT NULL DEFAULT '', row_version INTEGER NOT NULL DEFAULT 1, identity TEXT NOT NULL DEFAULT ''
) STRICT;
INSERT INTO subjects_new(id,kind,heading,thesaurus,subdivisions,name_parts,row_version)
  SELECT id,kind,heading,thesaurus,subdivisions,name_parts,row_version FROM subjects;
DROP TABLE subjects;
ALTER TABLE subjects_new RENAME TO subjects;
CREATE INDEX subjects_by_heading ON subjects(heading COLLATE NOCASE);
CREATE UNIQUE INDEX subjects_identity ON subjects(kind,heading,coalesce(thesaurus,0),subdivisions,identity);
CREATE TABLE IF NOT EXISTS import_identities (
   origin TEXT NOT NULL CHECK(length(trim(origin)) > 0),
   entity TEXT NOT NULL CHECK(entity = 'instance'),
   system TEXT NOT NULL,
   external_id TEXT NOT NULL,
   instance_id INTEGER NOT NULL REFERENCES instances(id) ON DELETE CASCADE,
   PRIMARY KEY(origin,entity,system,external_id,instance_id)
) STRICT, WITHOUT ROWID;
CREATE TABLE IF NOT EXISTS item_origins (
    origin TEXT NOT NULL, system TEXT NOT NULL, external_id TEXT NOT NULL,
    item_id INTEGER NOT NULL REFERENCES items(id) ON DELETE CASCADE,
    PRIMARY KEY(origin,system,external_id)
) STRICT, WITHOUT ROWID;
CREATE TABLE IF NOT EXISTS instance_origins (
     instance_id INTEGER NOT NULL REFERENCES instances(id) ON DELETE CASCADE,
     origin TEXT NOT NULL CHECK(length(trim(origin))>0),
     PRIMARY KEY(instance_id,origin)
) STRICT, WITHOUT ROWID;
CREATE TABLE IF NOT EXISTS note_visibility (
    instance_id INTEGER NOT NULL, position INTEGER NOT NULL, public INTEGER NOT NULL CHECK(public IN (0,1)),
    PRIMARY KEY(instance_id,position), FOREIGN KEY(instance_id,position) REFERENCES instance_notes(instance_id,position) ON DELETE CASCADE
) STRICT, WITHOUT ROWID;
CREATE TABLE IF NOT EXISTS item_issues (
    item_id INTEGER PRIMARY KEY REFERENCES items(id) ON DELETE CASCADE,
    enumeration TEXT, chronology TEXT, statement TEXT
) STRICT;
CREATE TABLE IF NOT EXISTS original_records (
    instance_id INTEGER PRIMARY KEY REFERENCES instances(id) ON DELETE CASCADE,
    original BLOB NOT NULL, declared TEXT NOT NULL, decoded TEXT NOT NULL, family TEXT NOT NULL, origin TEXT, serialization TEXT NOT NULL
) STRICT;
INSERT INTO effect_outbox(audit_seq,kind,payload)
SELECT (SELECT max(seq) FROM audit_log),'catalog.reindex_instance', json_object('instance',instance_id)
FROM instance_notes WHERE kind=3 AND EXISTS(SELECT 1 FROM audit_log) GROUP BY instance_id;

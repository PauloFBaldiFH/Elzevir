-- Undo migration 62 for fixtures that reconstruct an older catalog schema.
DROP TRIGGER repo_instance_retained;
DROP TABLE repo_versions;
DROP TABLE repo_files;
DROP TABLE repo_objects;
DROP TABLE repo_collection_members;
DROP TABLE repo_collections;
DROP TABLE repo_deposits;
-- SPDX-License-Identifier: GPL-3.0-or-later
UPDATE contributions SET agent_id = (SELECT min(b.id) FROM agents a JOIN agents b ON b.heading=a.heading AND b.kind=a.kind WHERE a.id=contributions.agent_id);
DELETE FROM agents WHERE id NOT IN (SELECT min(id) FROM agents GROUP BY kind,heading);
UPDATE work_subjects SET subject_id=(SELECT min(b.id) FROM subjects a JOIN subjects b ON a.kind=b.kind AND a.heading=b.heading AND a.thesaurus IS b.thesaurus AND a.subdivisions=b.subdivisions WHERE a.id=work_subjects.subject_id);
DELETE FROM subjects WHERE id NOT IN (SELECT min(id) FROM subjects GROUP BY kind,heading,thesaurus,subdivisions);
DROP TABLE instance_origins;
DROP TABLE original_records;
DROP TABLE item_origins;
DROP TABLE item_issues;
DROP TABLE note_visibility;
DROP TABLE import_identities;

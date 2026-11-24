-- SPDX-License-Identifier: GPL-3.0-or-later
CREATE TABLE repo_deposits (
    id BLOB PRIMARY KEY CHECK(length(id)=16),
    instance_id INTEGER NOT NULL UNIQUE REFERENCES instances(id) ON DELETE RESTRICT,
    state TEXT NOT NULL CHECK(state IN ('draft','published','withdrawn')),
    created_at INTEGER NOT NULL, published_at INTEGER, withdrawn_at INTEGER,
    row_version INTEGER NOT NULL DEFAULT 1 CHECK(row_version>=1),
    CHECK ((state='draft' AND published_at IS NULL AND withdrawn_at IS NULL)
        OR (state='published' AND published_at IS NOT NULL AND withdrawn_at IS NULL)
        OR (state='withdrawn' AND published_at IS NOT NULL AND withdrawn_at IS NOT NULL))
) STRICT;
CREATE INDEX repo_deposits_by_state ON repo_deposits(state,published_at,id);
CREATE TABLE repo_collections (
    id BLOB PRIMARY KEY CHECK(length(id)=16),
    name TEXT NOT NULL CHECK(length(trim(name)) BETWEEN 1 AND 256),
    description TEXT NOT NULL DEFAULT ''
) STRICT;
CREATE INDEX repo_collections_by_name ON repo_collections(name,id);
CREATE TABLE repo_collection_members (
    collection_id BLOB NOT NULL REFERENCES repo_collections(id) ON DELETE CASCADE,
    deposit_id BLOB NOT NULL REFERENCES repo_deposits(id) ON DELETE RESTRICT,
    PRIMARY KEY(collection_id,deposit_id)
) STRICT, WITHOUT ROWID;
CREATE INDEX repo_members_by_deposit ON repo_collection_members(deposit_id,collection_id);
CREATE TABLE repo_objects (
    sha256 BLOB PRIMARY KEY CHECK(length(sha256)=32),
    bytes INTEGER NOT NULL CHECK(bytes>0), created_at INTEGER NOT NULL,
    last_verified_at INTEGER,
    integrity TEXT NOT NULL CHECK(integrity IN ('ok','missing','corrupt'))
) STRICT;
CREATE INDEX repo_objects_by_verified ON repo_objects(last_verified_at,sha256);
CREATE TABLE repo_files (
    id BLOB PRIMARY KEY CHECK(length(id)=16),
    deposit_id BLOB NOT NULL REFERENCES repo_deposits(id) ON DELETE RESTRICT,
    role TEXT NOT NULL CHECK(role IN ('primary','supplement','administrative','label')),
    label TEXT NOT NULL CHECK(length(trim(label))>0),
    access TEXT NOT NULL CHECK(access IN ('public','staff')),
    position INTEGER NOT NULL DEFAULT 0 CHECK(position>=0),
    current_seq INTEGER CHECK(current_seq>=1), row_version INTEGER NOT NULL DEFAULT 1 CHECK(row_version>=1),
    CHECK(role!='administrative' OR access='staff'),
    FOREIGN KEY(id,current_seq) REFERENCES repo_versions(file_id,seq) ON DELETE RESTRICT
) STRICT;
CREATE INDEX repo_files_by_deposit ON repo_files(deposit_id,position,id);
CREATE UNIQUE INDEX repo_one_primary ON repo_files(deposit_id) WHERE role='primary';
CREATE TABLE repo_versions (
    file_id BLOB NOT NULL REFERENCES repo_files(id) ON DELETE RESTRICT,
    seq INTEGER NOT NULL CHECK(seq>=1),
    object_hash BLOB NOT NULL REFERENCES repo_objects(sha256) ON DELETE RESTRICT,
    display_name TEXT NOT NULL CHECK(length(trim(display_name))>0),
    media_type TEXT NOT NULL CHECK(length(trim(media_type))>0), created_at INTEGER NOT NULL,
    actor INTEGER NOT NULL, operation_id BLOB NOT NULL UNIQUE CHECK(length(operation_id)=16),
    expected_seq INTEGER CHECK(expected_seq>=1 AND expected_seq<seq), withdrawn_at INTEGER,
    PRIMARY KEY(file_id,seq)
) STRICT, WITHOUT ROWID;
CREATE INDEX repo_versions_by_hash ON repo_versions(object_hash);
CREATE TRIGGER repo_version_immutable BEFORE UPDATE ON repo_versions
WHEN OLD.withdrawn_at IS NOT NULL OR NEW.withdrawn_at IS NULL
 OR (NEW.file_id,NEW.seq,NEW.object_hash,NEW.display_name,NEW.media_type,NEW.created_at,
     NEW.actor,NEW.operation_id,NEW.expected_seq) IS NOT
    (OLD.file_id,OLD.seq,OLD.object_hash,OLD.display_name,OLD.media_type,OLD.created_at,
     OLD.actor,OLD.operation_id,OLD.expected_seq)
BEGIN SELECT RAISE(ABORT,'immutable_version'); END;
CREATE TRIGGER repo_instance_retained BEFORE DELETE ON instances
WHEN EXISTS(SELECT 1 FROM repo_deposits WHERE instance_id=OLD.id)
BEGIN SELECT RAISE(ABORT,'instance_has_deposit'); END;
CREATE TRIGGER repo_version_retained BEFORE DELETE ON repo_versions
BEGIN SELECT RAISE(ABORT,'retained_version'); END;
CREATE TRIGGER repo_current_is_latest BEFORE UPDATE OF current_seq ON repo_files
WHEN NEW.current_seq IS NOT NULL AND NOT EXISTS (
    SELECT 1 FROM repo_versions v WHERE v.file_id=NEW.id AND v.seq=NEW.current_seq
      AND v.withdrawn_at IS NULL
      AND v.seq=(SELECT max(seq) FROM repo_versions WHERE file_id=NEW.id))
BEGIN SELECT RAISE(ABORT,'invalid_current_version'); END;

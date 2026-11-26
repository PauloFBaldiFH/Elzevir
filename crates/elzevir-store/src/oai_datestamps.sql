-- SPDX-License-Identifier: GPL-3.0-or-later
-- When each Instance's version last moved, in UTC, for the OAI-PMH datestamp
-- (docs/specs/opac/oai-pmh.md, item 7). Triggers keep it in the writer's own transaction,
-- with the database's clock, as `audit_log.at` is; Instances already stored get the time
-- of the migration.
CREATE TABLE instance_datestamps (
    instance_id INTEGER PRIMARY KEY,
    changed_at  TEXT NOT NULL
) STRICT;
INSERT INTO instance_datestamps (instance_id, changed_at)
    SELECT id, strftime('%Y-%m-%dT%H:%M:%SZ', 'now') FROM instances;
CREATE INDEX instance_datestamps_by_time ON instance_datestamps (changed_at, instance_id);
CREATE TRIGGER instance_datestamp_added AFTER INSERT ON instances BEGIN
    INSERT OR REPLACE INTO instance_datestamps (instance_id, changed_at)
        VALUES (NEW.id, strftime('%Y-%m-%dT%H:%M:%SZ', 'now'));
END;
CREATE TRIGGER instance_datestamp_changed
    AFTER UPDATE OF row_version, opac_hidden ON instances BEGIN
    INSERT OR REPLACE INTO instance_datestamps (instance_id, changed_at)
        VALUES (NEW.id, strftime('%Y-%m-%dT%H:%M:%SZ', 'now'));
END;

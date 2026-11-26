-- SPDX-License-Identifier: GPL-3.0-or-later
-- Kind 3 of `koha_keys`: a returned loan brought from Koha's `old_issues`, by `issue_id`
-- (docs/specs/migracao/circulacao-do-koha.md, item 16), so a second run adds none twice.
CREATE TABLE koha_keys_new (
    kind     INTEGER NOT NULL CHECK (kind IN (1, 2, 3)),
    koha_key TEXT NOT NULL,
    local_id INTEGER NOT NULL,
    PRIMARY KEY (kind, koha_key)
) STRICT, WITHOUT ROWID;
INSERT INTO koha_keys_new (kind, koha_key, local_id) SELECT kind, koha_key, local_id FROM koha_keys;
DROP TABLE koha_keys;
ALTER TABLE koha_keys_new RENAME TO koha_keys;

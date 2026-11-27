-- SPDX-License-Identifier: GPL-3.0-or-later
-- 71: kinds 4 and 5 of `koha_keys`: a patron attribute type that became an additional
-- field (`code`), and an authorised value that became one of its options
-- (`code` and `authorised_value`, joined by the unit separator)
-- (docs/specs/circulacao/patron-attributes.md, item 16).
CREATE TABLE koha_keys_new (
    kind     INTEGER NOT NULL CHECK (kind IN (1, 2, 3, 4, 5)),
    koha_key TEXT NOT NULL,
    local_id INTEGER NOT NULL,
    PRIMARY KEY (kind, koha_key)
) STRICT, WITHOUT ROWID;
INSERT INTO koha_keys_new (kind, koha_key, local_id) SELECT kind, koha_key, local_id FROM koha_keys;
DROP TABLE koha_keys;
ALTER TABLE koha_keys_new RENAME TO koha_keys;

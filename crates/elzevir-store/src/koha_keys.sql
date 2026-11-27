-- SPDX-License-Identifier: GPL-3.0-or-later
-- The keys of what came from Koha by the patron migration
-- (docs/specs/migracao/leitores-do-koha.md, items 7 and 13), kept at the edge like
-- `koha_codes`: kind 1 is a patron category (`categorycode`), kind 2 a patron
-- (`borrowernumber`). The core never reads it.
CREATE TABLE koha_keys (
    kind     INTEGER NOT NULL CHECK (kind IN (1, 2)),
    koha_key TEXT NOT NULL,
    local_id INTEGER NOT NULL,
    PRIMARY KEY (kind, koha_key)
) STRICT, WITHOUT ROWID;

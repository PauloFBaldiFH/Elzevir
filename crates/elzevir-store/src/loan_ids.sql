-- SPDX-License-Identifier: GPL-3.0-or-later
-- 74: a loan id is never given again. SQLite gives a deleted largest id to the next row,
-- so an ended loan's id came back with the next checkout, and the debits, fines and
-- returned loan of the first loan seemed the second's: losing an Item twice charged it
-- once (docs/specs/circulacao/lost-items.md, edge case 2).
CREATE TABLE loans_new (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    item_id    INTEGER NOT NULL UNIQUE REFERENCES items (id),
    patron_id  INTEGER NOT NULL REFERENCES patrons (id),
    branch_id  INTEGER NOT NULL REFERENCES branches (id),
    issued_at  TEXT NOT NULL,
    due        TEXT NOT NULL,
    on_site    INTEGER NOT NULL CHECK (on_site IN (0, 1)),
    renewals   INTEGER NOT NULL DEFAULT 0,
    renewed_at TEXT
);
INSERT INTO loans_new (id, item_id, patron_id, branch_id, issued_at, due, on_site, renewals,
                       renewed_at)
SELECT id, item_id, patron_id, branch_id, issued_at, due, on_site, renewals, renewed_at
FROM loans;
DROP TABLE loans;
ALTER TABLE loans_new RENAME TO loans;
CREATE INDEX loans_by_patron ON loans (patron_id);
-- The next id is above every id a loan, a debit or a returned loan names already.
DELETE FROM sqlite_sequence WHERE name = 'loans';
INSERT INTO sqlite_sequence (name, seq)
SELECT 'loans', max(coalesce((SELECT max(id) FROM loans), 0),
                    coalesce((SELECT max(loan_id) FROM account_lines), 0),
                    coalesce((SELECT max(loan_id) FROM returned_loans), 0));

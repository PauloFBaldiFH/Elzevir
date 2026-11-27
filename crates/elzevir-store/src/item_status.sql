-- SPDX-License-Identifier: GPL-3.0-or-later
-- 72: the names a library gives the status numbers of its Items, and when each status of
-- an Item last went from 0 to a number (docs/specs/circulacao/lost-items.md, items 1
-- and 2).
CREATE TABLE item_status_names (
    kind   TEXT NOT NULL CHECK (kind IN ('lost', 'damaged', 'withdrawn', 'not_for_loan')),
    number INTEGER NOT NULL CHECK (number BETWEEN 1 AND 99),
    label  TEXT NOT NULL,
    PRIMARY KEY (kind, number),
    UNIQUE (kind, label)
) STRICT;
ALTER TABLE items ADD COLUMN lost_on TEXT;
ALTER TABLE items ADD COLUMN damaged_on TEXT;
ALTER TABLE items ADD COLUMN withdrawn_on TEXT;

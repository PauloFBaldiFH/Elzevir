-- SPDX-License-Identifier: GPL-3.0-or-later
-- 73: a returned loan keeps the id it had while open, which its debits name in
-- `account_lines.loan_id`, so that finding a lost Item can tell whether its charge came
-- from the Item's last loan (docs/specs/circulacao/lost-items.md, item 13).
ALTER TABLE returned_loans ADD COLUMN loan_id INTEGER;

-- 68: what the reports count after a loan ends (docs/specs/operacao/relatorios.md,
-- items 2 and 9). An ended loan keeps the category its patron had, even once the patron
-- is dropped from it, as Koha's statistics table does; a patron keeps the day of the
-- latest checkout, as Koha's borrowers.lastseen does for an issue.
ALTER TABLE returned_loans ADD COLUMN category_id INTEGER REFERENCES patron_categories (id);
UPDATE returned_loans SET category_id =
    (SELECT category_id FROM patrons WHERE patrons.id = returned_loans.patron_id);
ALTER TABLE patrons ADD COLUMN last_issued_on TEXT;
UPDATE patrons SET last_issued_on = (
    SELECT max(day) FROM (
        SELECT substr(issued_at, 1, 10) AS day FROM loans WHERE patron_id = patrons.id
        UNION ALL
        SELECT substr(issued_at, 1, 10) FROM returned_loans WHERE patron_id = patrons.id));

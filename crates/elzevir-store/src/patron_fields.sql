-- 70: additional patron fields the library defines, their options and the values patrons
-- hold (docs/specs/circulacao/patron-attributes.md). Kind 1 is text, 2 a date, 3 a list.
-- A value is an option of a list field, or a text (a date in ISO form) otherwise.
CREATE TABLE patron_fields (
    id          INTEGER PRIMARY KEY,
    name        TEXT NOT NULL UNIQUE,
    kind        INTEGER NOT NULL CHECK (kind IN (1, 2, 3)),
    repeatable  INTEGER NOT NULL CHECK (repeatable IN (0, 1)),
    is_unique   INTEGER NOT NULL CHECK (is_unique IN (0, 1)),
    required    INTEGER NOT NULL CHECK (required IN (0, 1)),
    category_id INTEGER REFERENCES patron_categories (id),
    opac_shown  INTEGER NOT NULL CHECK (opac_shown IN (0, 1)),
    searchable  INTEGER NOT NULL CHECK (searchable IN (0, 1)),
    row_version INTEGER NOT NULL DEFAULT 1
) STRICT;
CREATE TABLE patron_field_options (
    id       INTEGER PRIMARY KEY,
    field_id INTEGER NOT NULL REFERENCES patron_fields (id),
    label    TEXT NOT NULL,
    position INTEGER NOT NULL,
    UNIQUE (field_id, label)
) STRICT;
CREATE TABLE patron_field_values (
    id        INTEGER PRIMARY KEY,
    patron_id INTEGER NOT NULL REFERENCES patrons (id),
    field_id  INTEGER NOT NULL REFERENCES patron_fields (id),
    option_id INTEGER REFERENCES patron_field_options (id),
    text      TEXT,
    CHECK ((option_id IS NULL) <> (text IS NULL))
) STRICT;
CREATE INDEX patron_field_values_by_patron ON patron_field_values (patron_id, field_id);
CREATE INDEX patron_field_values_by_field ON patron_field_values (field_id, option_id, text);

// SPDX-License-Identifier: GPL-3.0-or-later
//! Connection setup and migrations.
//!
//! Migrations are plain SQL, applied in order by the writer connection at open time and
//! tracked in `PRAGMA user_version`. A migration is never edited after it ships; changes
//! are new migrations. Every table with rows that users edit carries a `row_version`
//! column (docs/arquitetura/05-definicoes-de-implementacao.md, §3).

use std::path::Path;
use std::time::Duration;

use elzevir_app::description::concept;
use elzevir_app::listing::sort_key;
use elzevir_app::marc_mapping::{
    source_admin, source_audience, source_books, source_classification, source_content,
    source_contributions, source_description, source_extent, source_fixed, source_forms,
    source_leader_conventions, source_notes, source_opac_hidden, source_periods_and_numbers,
    source_physical, source_provision, source_publication, source_relations, source_responsibility,
    source_subjects, source_titles, source_typed_notes,
};
use elzevir_app::matching::{identifier_key, koha_biblionumber_key};
use elzevir_domain::catalog::IdentifierKind;
use elzevir_domain::catalog::SubjectKind;
use elzevir_domain::statement::{Description, EntityKind, EntityRef, Statement, Value};
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};

use crate::catalog::{
    insert_contribution_identifiers, insert_notes, insert_subject_identifiers, set_name_parts,
    subject_id,
};
use crate::statements::{read_description, store_description};
use crate::{CommandFailed, StoreError};

/// Ordered migrations; migration `n` (1-based) moves `user_version` from `n - 1` to `n`.
const MIGRATIONS: &[&str] = &[
    // 1: infrastructure tables.
    "CREATE TABLE audit_log (
         seq        INTEGER PRIMARY KEY,
         kind       TEXT NOT NULL,
         command    TEXT NOT NULL CHECK (json_valid(command))
     ) STRICT;
     CREATE TABLE effect_outbox (
         id         INTEGER PRIMARY KEY,
         audit_seq  INTEGER NOT NULL REFERENCES audit_log (seq),
         kind       TEXT NOT NULL,
         payload    TEXT NOT NULL CHECK (json_valid(payload)),
         done       INTEGER NOT NULL DEFAULT 0 CHECK (done IN (0, 1))
     ) STRICT;
     CREATE INDEX effect_outbox_pending ON effect_outbox (id) WHERE done = 0;",
    // 2: staff accounts and their permissions.
    "CREATE TABLE staff_accounts (
         id            INTEGER PRIMARY KEY,
         login         TEXT NOT NULL UNIQUE,
         password_hash TEXT NOT NULL,
         row_version   INTEGER NOT NULL DEFAULT 1
     ) STRICT;
     CREATE TABLE staff_permissions (
         staff_id   INTEGER NOT NULL REFERENCES staff_accounts (id) ON DELETE CASCADE,
         permission TEXT NOT NULL,
         PRIMARY KEY (staff_id, permission)
     ) STRICT, WITHOUT ROWID;",
    // 3: the bibliographic model (docs/specs/catalogo/modelo-bibliografico.md). Small
    // integer columns named `kind`, `vocabulary` or `format` hold the stable numbers of
    // the enums in `elzevir_domain::catalog`.
    "CREATE TABLE branches (
         id          INTEGER PRIMARY KEY,
         code        TEXT NOT NULL UNIQUE,
         name        TEXT NOT NULL,
         row_version INTEGER NOT NULL DEFAULT 1
     ) STRICT;
     CREATE TABLE item_types (
         id          INTEGER PRIMARY KEY,
         code        TEXT NOT NULL UNIQUE,
         description TEXT NOT NULL,
         row_version INTEGER NOT NULL DEFAULT 1
     ) STRICT;
     CREATE TABLE terms (
         id          INTEGER PRIMARY KEY,
         vocabulary  INTEGER NOT NULL CHECK (vocabulary BETWEEN 1 AND 255),
         code        TEXT NOT NULL,
         label       TEXT NOT NULL,
         UNIQUE (vocabulary, code)
     ) STRICT;
     CREATE TABLE agents (
         id          INTEGER PRIMARY KEY,
         kind        INTEGER NOT NULL CHECK (kind BETWEEN 1 AND 255),
         heading     TEXT NOT NULL,
         UNIQUE (kind, heading)
     ) STRICT;
     CREATE TABLE subjects (
         id          INTEGER PRIMARY KEY,
         kind        INTEGER NOT NULL CHECK (kind BETWEEN 1 AND 255),
         heading     TEXT NOT NULL,
         thesaurus   INTEGER REFERENCES terms (id)
     ) STRICT;
     -- Two headings without a thesaurus are the same subject; a plain UNIQUE would let
     -- them both in, because NULLs never collide.
     CREATE UNIQUE INDEX subjects_identity ON subjects (kind, heading, coalesce(thesaurus, 0));
     CREATE TABLE works (
         id             INTEGER PRIMARY KEY,
         title          TEXT NOT NULL,
         classification TEXT,
         classification_scheme INTEGER REFERENCES terms (id),
         summary        TEXT,
         extra          TEXT NOT NULL DEFAULT '{}' CHECK (json_valid(extra)),
         row_version    INTEGER NOT NULL DEFAULT 1
     ) STRICT;
     CREATE TABLE work_languages (
         work_id   INTEGER NOT NULL REFERENCES works (id) ON DELETE CASCADE,
         position  INTEGER NOT NULL,
         language  INTEGER NOT NULL REFERENCES terms (id),
         PRIMARY KEY (work_id, position)
     ) STRICT, WITHOUT ROWID;
     CREATE TABLE contributions (
         work_id   INTEGER NOT NULL REFERENCES works (id) ON DELETE CASCADE,
         position  INTEGER NOT NULL,
         agent_id  INTEGER NOT NULL REFERENCES agents (id),
         role      INTEGER REFERENCES terms (id),
         is_primary INTEGER NOT NULL DEFAULT 0 CHECK (is_primary IN (0, 1)),
         PRIMARY KEY (work_id, position)
     ) STRICT, WITHOUT ROWID;
     CREATE INDEX contributions_by_agent ON contributions (agent_id);
     CREATE TABLE work_subjects (
         work_id    INTEGER NOT NULL REFERENCES works (id) ON DELETE CASCADE,
         position   INTEGER NOT NULL,
         subject_id INTEGER NOT NULL REFERENCES subjects (id),
         PRIMARY KEY (work_id, position)
     ) STRICT, WITHOUT ROWID;
     CREATE INDEX work_subjects_by_subject ON work_subjects (subject_id);
     CREATE TABLE instances (
         id                 INTEGER PRIMARY KEY,
         work_id            INTEGER NOT NULL REFERENCES works (id),
         title              TEXT NOT NULL,
         edition            TEXT,
         place              TEXT,
         publisher          TEXT,
         date               TEXT,
         extent             TEXT,
         series             TEXT,
         extra              TEXT NOT NULL DEFAULT '{}' CHECK (json_valid(extra)),
         row_version        INTEGER NOT NULL DEFAULT 1
     ) STRICT;
     CREATE INDEX instances_by_work ON instances (work_id);
     CREATE TABLE instance_identifiers (
         instance_id INTEGER NOT NULL REFERENCES instances (id) ON DELETE CASCADE,
         position    INTEGER NOT NULL,
         kind        INTEGER NOT NULL CHECK (kind BETWEEN 1 AND 255),
         value       TEXT NOT NULL,
         qualifier   TEXT,
         PRIMARY KEY (instance_id, position)
     ) STRICT, WITHOUT ROWID;
     CREATE INDEX instance_identifiers_by_value ON instance_identifiers (kind, value);
     CREATE TABLE source_records (
         instance_id INTEGER PRIMARY KEY REFERENCES instances (id) ON DELETE CASCADE,
         format      INTEGER NOT NULL CHECK (format BETWEEN 1 AND 255),
         record      BLOB NOT NULL
     ) STRICT;
     CREATE TABLE items (
         id                INTEGER PRIMARY KEY,
         instance_id       INTEGER NOT NULL REFERENCES instances (id),
         barcode           TEXT UNIQUE,
         home_branch       INTEGER NOT NULL REFERENCES branches (id),
         holding_branch    INTEGER NOT NULL REFERENCES branches (id),
         item_type         INTEGER NOT NULL REFERENCES item_types (id),
         call_number       TEXT,
         location          INTEGER REFERENCES terms (id),
         collection        INTEGER REFERENCES terms (id),
         copy_number       TEXT,
         not_for_loan      INTEGER NOT NULL DEFAULT 0,
         damaged           INTEGER NOT NULL DEFAULT 0,
         lost              INTEGER NOT NULL DEFAULT 0,
         withdrawn         INTEGER NOT NULL DEFAULT 0,
         restricted        INTEGER NOT NULL DEFAULT 0,
         price_cents       INTEGER,
         replacement_price_cents INTEGER,
         acquired_on       TEXT,
         extra             TEXT NOT NULL DEFAULT '{}' CHECK (json_valid(extra)),
         row_version       INTEGER NOT NULL DEFAULT 1
     ) STRICT;
     CREATE INDEX items_by_instance ON items (instance_id);",
    // 4: Hubs, relations between Works, material types and inventory numbers (ADR 0002).
    "CREATE TABLE hubs (
         id          INTEGER PRIMARY KEY,
         title       TEXT NOT NULL,
         extra       TEXT NOT NULL DEFAULT '{}' CHECK (json_valid(extra)),
         row_version INTEGER NOT NULL DEFAULT 1
     ) STRICT;
     ALTER TABLE works ADD COLUMN hub_id INTEGER REFERENCES hubs (id);
     CREATE INDEX works_by_hub ON works (hub_id) WHERE hub_id IS NOT NULL;
     ALTER TABLE works ADD COLUMN material_type INTEGER;
     ALTER TABLE instances ADD COLUMN material_type INTEGER;
     CREATE TABLE work_relations (
         work_id         INTEGER NOT NULL REFERENCES works (id) ON DELETE CASCADE,
         position        INTEGER NOT NULL,
         kind            INTEGER NOT NULL,
         related_work_id INTEGER NOT NULL REFERENCES works (id),
         CHECK (work_id <> related_work_id),
         PRIMARY KEY (work_id, position)
     ) STRICT, WITHOUT ROWID;
     CREATE INDEX work_relations_by_related ON work_relations (related_work_id);
     ALTER TABLE items ADD COLUMN inventory_number TEXT;
     CREATE INDEX items_by_inventory_number ON items (inventory_number)
         WHERE inventory_number IS NOT NULL;",
    // 5: match keys for importing without duplicates (docs/specs/catalogo/casamento.md).
    // The keys of records stored before this migration are filled in by
    // `backfill_match_keys`, because computing them needs Rust (ISBN-13, MARC 999).
    "CREATE TABLE match_keys (
         rule        INTEGER NOT NULL CHECK (rule BETWEEN 1 AND 255),
         key         TEXT NOT NULL,
         instance_id INTEGER NOT NULL REFERENCES instances (id) ON DELETE CASCADE,
         PRIMARY KEY (rule, key, instance_id)
     ) STRICT, WITHOUT ROWID;
     CREATE INDEX match_keys_by_instance ON match_keys (instance_id);",
    // 6: notes of an Instance (MARC21 500, 505, 590), task 2.1.
    "CREATE TABLE instance_notes (
         instance_id INTEGER NOT NULL REFERENCES instances (id) ON DELETE CASCADE,
         position    INTEGER NOT NULL,
         kind        INTEGER NOT NULL CHECK (kind BETWEEN 1 AND 255),
         text        TEXT NOT NULL,
         PRIMARY KEY (instance_id, position)
     ) STRICT, WITHOUT ROWID;",
    // 7: target audience of a Work (MARC21 521), task 2.1.
    "ALTER TABLE works ADD COLUMN audience TEXT;",
    // 8: listing order of families (Hubs and Works outside a Hub), task 2.3.
    "ALTER TABLE works ADD COLUMN sort_key TEXT NOT NULL DEFAULT '';
     ALTER TABLE hubs ADD COLUMN sort_key TEXT NOT NULL DEFAULT '';
     CREATE INDEX works_in_listing ON works (sort_key, id) WHERE hub_id IS NULL;
     CREATE INDEX hubs_in_listing ON hubs (sort_key, id);",
    // 9: the staff account a command comes from (docs/specs/catalogo/edicao.md, item 18).
    // No foreign key: the log keeps who did what after the account is gone.
    "ALTER TABLE audit_log ADD COLUMN actor INTEGER;",
    // 10: name lookups for the cataloguing suggestions (docs/specs/catalogo/edicao.md, item 37).
    "CREATE INDEX agents_by_heading ON agents (heading COLLATE NOCASE);
     CREATE INDEX subjects_by_heading ON subjects (heading COLLATE NOCASE);",
    // 11: BIBFRAME statements (docs/specs/catalogo/bibframe-completo.md, items 10a to 10g).
    // A top-level statement names its subject (1 Hub, 2 Work, 3 Instance, 4 Item); a
    // nested one names the statement whose resource it describes.
    "CREATE TABLE ontology_names (
         id    INTEGER PRIMARY KEY,
         name  TEXT NOT NULL UNIQUE
     ) STRICT;
     CREATE TABLE lc_concepts (
         id      INTEGER PRIMARY KEY,
         scheme  TEXT NOT NULL,
         code    TEXT NOT NULL,
         UNIQUE (scheme, code)
     ) STRICT;
     CREATE TABLE statements (
         id            INTEGER PRIMARY KEY,
         subject_kind  INTEGER CHECK (subject_kind BETWEEN 1 AND 4),
         subject_id    INTEGER,
         parent        INTEGER REFERENCES statements (id) ON DELETE CASCADE,
         position      INTEGER NOT NULL,
         property      INTEGER NOT NULL REFERENCES ontology_names (id),
         literal       TEXT,
         language      TEXT CHECK (language IS NULL OR literal IS NOT NULL),
         concept       INTEGER REFERENCES lc_concepts (id),
         entity_kind   INTEGER CHECK (entity_kind BETWEEN 1 AND 4),
         entity_id     INTEGER CHECK ((entity_id IS NULL) = (entity_kind IS NULL)),
         iri           TEXT,
         class         INTEGER REFERENCES ontology_names (id),
         label         TEXT CHECK (label IS NULL OR class IS NOT NULL),
         CHECK ((parent IS NULL) = (subject_kind IS NOT NULL AND subject_id IS NOT NULL)),
         CHECK ((literal IS NOT NULL) + (concept IS NOT NULL) + (entity_kind IS NOT NULL)
                + (iri IS NOT NULL) + (class IS NOT NULL) = 1)
     ) STRICT;
     CREATE INDEX statements_of_subject ON statements (subject_kind, subject_id, position)
         WHERE parent IS NULL;
     CREATE INDEX statements_of_parent ON statements (parent, position) WHERE parent IS NOT NULL;",
    // 12: the classes of each entity (bibframe-completo.md, item 10h).
    "CREATE TABLE entity_classes (
         subject_kind  INTEGER NOT NULL CHECK (subject_kind BETWEEN 1 AND 4),
         subject_id    INTEGER NOT NULL,
         position      INTEGER NOT NULL,
         class         INTEGER NOT NULL REFERENCES ontology_names (id),
         PRIMARY KEY (subject_kind, subject_id, class)
     ) STRICT, WITHOUT ROWID;",
    // 13: `extra` gives way to the statements (bibframe-completo.md, item 7). Nothing ever
    // wrote it; `check_extra_is_empty` refuses to drop a column that holds data.
    "ALTER TABLE hubs DROP COLUMN extra;
     ALTER TABLE works DROP COLUMN extra;
     ALTER TABLE instances DROP COLUMN extra;
     ALTER TABLE items DROP COLUMN extra;",
    // 14: registries of sublocations and collections per branch, contact data of the
    // branches, and the edge table of Koha codes (bibframe-completo.md, items 23a, 23b).
    "ALTER TABLE branches ADD COLUMN address TEXT;
     ALTER TABLE branches ADD COLUMN phone TEXT;
     ALTER TABLE branches ADD COLUMN email TEXT;
     ALTER TABLE branches ADD COLUMN url TEXT;
     CREATE TABLE sublocations (
         id          INTEGER PRIMARY KEY,
         branch_id   INTEGER NOT NULL REFERENCES branches (id),
         name        TEXT NOT NULL,
         row_version INTEGER NOT NULL DEFAULT 1,
         UNIQUE (branch_id, name)
     ) STRICT;
     CREATE TABLE collections (
         id          INTEGER PRIMARY KEY,
         branch_id   INTEGER NOT NULL REFERENCES branches (id),
         name        TEXT NOT NULL,
         row_version INTEGER NOT NULL DEFAULT 1,
         UNIQUE (branch_id, name)
     ) STRICT;
     CREATE TABLE koha_codes (
         kind      INTEGER NOT NULL CHECK (kind BETWEEN 1 AND 4),
         local_id  INTEGER NOT NULL,
         code      TEXT NOT NULL,
         PRIMARY KEY (kind, local_id)
     ) STRICT, WITHOUT ROWID;
     CREATE INDEX koha_codes_by_code ON koha_codes (kind, code);
     ALTER TABLE items ADD COLUMN sublocation_id INTEGER REFERENCES sublocations (id);
     ALTER TABLE items ADD COLUMN collection_id INTEGER REFERENCES collections (id);",
    // 15: codes leave the core (items 23a, 23b): a branch is its name, an Item points at
    // the registries, and the location and collection terms go. `migrate` turns foreign
    // keys off around it, since `branches` is rebuilt under the tables that reference it.
    "CREATE TABLE branches_without_code (
         id          INTEGER PRIMARY KEY,
         name        TEXT NOT NULL,
         row_version INTEGER NOT NULL DEFAULT 1,
         address     TEXT,
         phone       TEXT,
         email       TEXT,
         url         TEXT
     ) STRICT;
     INSERT INTO branches_without_code (id, name, row_version, address, phone, email, url)
         SELECT id, name, row_version, address, phone, email, url FROM branches;
     DROP TABLE branches;
     ALTER TABLE branches_without_code RENAME TO branches;
     ALTER TABLE items DROP COLUMN location;
     ALTER TABLE items DROP COLUMN collection;
     DELETE FROM terms WHERE vocabulary IN (3, 4);",
    // 16: circulation policies take the place of item types (item 23c). Each item type
    // becomes a policy with the same id and its name; the type code goes to the edge.
    // Renaming the table first carries the reference of `items` along; the rebuild then
    // drops `code`. Foreign keys are off, as for migration 15.
    "INSERT OR REPLACE INTO koha_codes (kind, local_id, code) SELECT 4, id, code FROM item_types;
     ALTER TABLE item_types RENAME TO circulation_policies;
     CREATE TABLE circulation_policies_without_code (
         id          INTEGER PRIMARY KEY,
         name        TEXT NOT NULL,
         row_version INTEGER NOT NULL DEFAULT 1
     ) STRICT;
     INSERT INTO circulation_policies_without_code (id, name, row_version)
         SELECT id, description, row_version FROM circulation_policies;
     DROP TABLE circulation_policies;
     ALTER TABLE circulation_policies_without_code RENAME TO circulation_policies;
     ALTER TABLE items RENAME COLUMN item_type TO policy_id;",
    // 17: a library's translation of its Koha item types into classes and coded
    // statements, kept at the MARC edge (item 23e).
    "CREATE TABLE koha_type_statements (
         code      TEXT NOT NULL,
         entity    INTEGER NOT NULL CHECK (entity BETWEEN 1 AND 2),
         position  INTEGER NOT NULL,
         property  TEXT NOT NULL,
         scheme    TEXT NOT NULL,
         value     TEXT NOT NULL,
         PRIMARY KEY (code, entity, position)
     ) STRICT, WITHOUT ROWID;",
    // 18: titles other than the preferred one become statements (item 29, part 5a); the
    // migration only fills them from the stored source records.
    "SELECT 1;",
    // 19: notes with a type, custodial history and frequency become statements (item 29,
    // part 5b); the migration only fills them from the stored source records.
    "SELECT 1;",
    // 20: production, distribution, manufacture and copyright dates become statements, and
    // every place and publisher of the publication is kept (item 29, part 5c); the
    // migration only fills them from the stored source records.
    "SELECT 1;",
    // 21: series and related works become relations of the Work (item 29, part 5d); the
    // migration only fills them from the stored source records.
    "SELECT 1;", // 22: geographic coverage, languages of the parts, scripts and electronic locations
    // become statements (item 29, part 5e); the migration only fills them from the stored
    // source records.
    "SELECT 1;",
    // 23: the extent keeps `300 $a $f $g`; physical details, dimensions and accompanying
    // material become statements (item 29, part 5e). The migration fills them from the
    // stored source records.
    "SELECT 1;",
    // 24: the administrative metadata (Leader/05, `001`, `003`, `005`, `008/00-05`, `040`)
    // becomes a statement of the Instance (item 29, part 5e); the migration only fills it
    // from the stored source records.
    "SELECT 1;",
    // 25: the coded publication of `008` (type of date, dates, country) becomes statements
    // (item 29, part 5e); the migration only fills them from the stored source records.
    "SELECT 1;",
    // 26: the statement of responsibility (`245 $c`) becomes a statement of the Instance
    // (item 29, part 5f); the migration only fills it from the stored source records.
    "SELECT 1;",
    // 27: a subject keeps what each subdivision names (item 29, part 5f), as the codes of
    // its kinds separated by commas; two headings with other subdivisions are two subjects.
    // The migration fills them from the stored source records.
    "ALTER TABLE subjects ADD COLUMN subdivisions TEXT NOT NULL DEFAULT '';
     DROP INDEX subjects_identity;
     CREATE UNIQUE INDEX subjects_identity
         ON subjects (kind, heading, coalesce(thesaurus, 0), subdivisions);",
    // 28: an agent keeps where each part of its heading after the first starts (item 29,
    // part 5f), as `kind@start` pairs separated by commas. The migration fills them from
    // the stored source records.
    "ALTER TABLE agents ADD COLUMN name_parts TEXT NOT NULL DEFAULT '';",
    // 29: a contribution keeps the identifiers its record gives the agent (`$0`, `$1`) and
    // the relator term as written beside the code (item 29, part 5f). They are the record's:
    // records of one agent do not all link it. The migration fills them from the stored
    // source records.
    "CREATE TABLE contribution_identifiers (
         work_id       INTEGER NOT NULL,
         contribution  INTEGER NOT NULL,
         position      INTEGER NOT NULL,
         kind          INTEGER NOT NULL CHECK (kind BETWEEN 1 AND 255),
         value         TEXT NOT NULL,
         PRIMARY KEY (work_id, contribution, position),
         FOREIGN KEY (work_id, contribution) REFERENCES contributions (work_id, position)
             ON DELETE CASCADE
     ) STRICT, WITHOUT ROWID;
     ALTER TABLE contributions ADD COLUMN role_term TEXT;",
    // 30: a subject that names an agent says which kind (600 person or family, 610
    // organization, 611 meeting) and keeps the parts of its name; every subject keeps the
    // links its record gives the heading (item 29, part 5f). Kind 3 was any agent and is
    // now a person; the migration reads the other kinds, the parts and the links from the
    // stored source records.
    "ALTER TABLE subjects ADD COLUMN name_parts TEXT NOT NULL DEFAULT '';
     CREATE TABLE subject_identifiers (
         work_id   INTEGER NOT NULL,
         subject   INTEGER NOT NULL,
         position  INTEGER NOT NULL,
         kind      INTEGER NOT NULL CHECK (kind BETWEEN 1 AND 255),
         value     TEXT NOT NULL,
         PRIMARY KEY (work_id, subject, position),
         FOREIGN KEY (work_id, subject) REFERENCES work_subjects (work_id, position)
             ON DELETE CASCADE
     ) STRICT, WITHOUT ROWID;",
    // 31: the description conventions of Leader/18 (AACR 2, ISBD) join the administrative
    // metadata (item 29, part 5f); the migration only fills them from the stored source
    // records.
    "SELECT 1;",
    // 32: every classification field (`050`, `080`, `082`, `084`) becomes a statement of the
    // Work (item 29, part 5f), beside the main number the Work keeps; the migration only fills
    // them from the stored source records.
    "SELECT 1;",
    // 33: the positions of a book in `008/18-34` (illustrations, coded audience, nature of
    // the contents, conference, festschrift, index, literary form, biography) become
    // statements of the Work (item 29, part 5f); the migration only fills them from the
    // stored source records.
    "SELECT 1;",
    // 34: the time periods of `045` become statements of the Work and the numbers of
    // national bibliographic agencies of `016` statements of the Instance (item 29, part
    // 5f); the migration only fills them from the stored source records.
    "SELECT 1;",
    // 35: the forms of the Work of `380` become `bf:genreForm` statements (item 29, part
    // 5f); the migration only fills them from the stored source records.
    "SELECT 1;",
    // 36: the category of material leaves (bibframe-completo.md, item 14b). An Instance
    // whose own category was digital (7) under a Work that was not keeps it as the class
    // `Electronic`, as the import now gives it; then the columns go.
    "INSERT OR IGNORE INTO ontology_names (name) VALUES ('Electronic');
     INSERT OR IGNORE INTO entity_classes (subject_kind, subject_id, position, class)
         SELECT 3, i.id,
                COALESCE((SELECT MAX(position) + 1 FROM entity_classes
                          WHERE subject_kind = 3 AND subject_id = i.id), 0),
                (SELECT id FROM ontology_names WHERE name = 'Electronic')
         FROM instances i JOIN works w ON w.id = i.work_id
         WHERE i.material_type = 7 AND w.material_type IS NOT 7;
     ALTER TABLE works DROP COLUMN material_type;
     ALTER TABLE instances DROP COLUMN material_type;",
    // 37: the history of an entity (docs/specs/catalogo/historico.md): when each command
    // ran, and which Works and Instances it touched. Older edits of a Work or an Instance
    // are found from their commands; when they ran was not kept.
    "ALTER TABLE audit_log ADD COLUMN at TEXT;
     CREATE TABLE audit_entities (
         entity_kind INTEGER NOT NULL,
         entity_id   INTEGER NOT NULL,
         audit_seq   INTEGER NOT NULL REFERENCES audit_log (seq),
         PRIMARY KEY (entity_kind, entity_id, audit_seq)
     ) STRICT, WITHOUT ROWID;
     INSERT OR IGNORE INTO audit_entities (entity_kind, entity_id, audit_seq)
         SELECT 2, json_extract(command, '$.Catalog.UpdateWork.work'), seq
         FROM audit_log WHERE kind = 'catalog.update_work'
         UNION ALL
         SELECT 3, json_extract(command, '$.Catalog.UpdateInstance.instance'), seq
         FROM audit_log WHERE kind = 'catalog.update_instance'
         UNION ALL
         SELECT 3, json_extract(command, '$.Catalog.SaveItems.instance'), seq
         FROM audit_log WHERE kind = 'catalog.save_items';",
    // 38: an Instance can be hidden from the OPAC (docs/specs/catalogo/ocultar-no-opac.md);
    // the records already stored take it from their `942 $n`.
    "ALTER TABLE instances ADD COLUMN opac_hidden INTEGER NOT NULL DEFAULT 0
         CHECK (opac_hidden IN (0, 1));",
    // 39: an agent is edited once for every record that names it
    // (docs/specs/catalogo/autoridades.md); an edit names the version it read.
    "ALTER TABLE agents ADD COLUMN row_version INTEGER NOT NULL DEFAULT 1;",
    // 40: a subject is edited once for every record that names it, as an agent is.
    "ALTER TABLE subjects ADD COLUMN row_version INTEGER NOT NULL DEFAULT 1;",
    // 41: the work lists read a person's commands, and the imports of a day, from the
    // newest (docs/specs/catalogo/facetas.md, items 15 and 16).
    "CREATE INDEX audit_by_actor ON audit_log (actor, seq) WHERE actor IS NOT NULL;
     CREATE INDEX audit_by_kind ON audit_log (kind, at);
     CREATE INDEX audit_entities_by_seq ON audit_entities (audit_seq);",
    // 42: lists of families the team saves (docs/specs/catalogo/lote.md, part c). A
    // family is kept by its head as the listing writes it (`h12`, `w7`); `added` orders
    // a list, the last added first.
    "CREATE TABLE saved_lists (
         id          INTEGER PRIMARY KEY,
         name        TEXT NOT NULL UNIQUE COLLATE NOCASE,
         row_version INTEGER NOT NULL DEFAULT 1
     ) STRICT;
     CREATE TABLE saved_list_families (
         list_id INTEGER NOT NULL REFERENCES saved_lists (id),
         head    TEXT NOT NULL,
         added   INTEGER NOT NULL,
         PRIMARY KEY (list_id, head)
     ) STRICT, WITHOUT ROWID;
     CREATE INDEX saved_list_families_by_added ON saved_list_families (list_id, added);",
    // 43: the imports from a file, newest first, without reading each command's JSON at
    // every page of "imported today" (docs/specs/catalogo/facetas.md, item 16). The
    // condition must stay word for word the one `worklist::CHOSEN_IMPORTS` names.
    "CREATE INDEX audit_imports ON audit_log (at)
     WHERE kind = 'catalog.import_record'
        OR (kind = 'catalog.add_record'
            AND json_extract(command, '$.Catalog.AddRecord.source') IS NOT NULL);",
    // 44: patrons and their categories (docs/specs/circulacao/leitores.md). A category
    // has exactly one of the two periods. Dates are ISO text, as everywhere else.
    "CREATE TABLE patron_categories (
         id               INTEGER PRIMARY KEY,
         name             TEXT NOT NULL UNIQUE,
         enrolment_months INTEGER,
         enrolment_until  TEXT,
         row_version      INTEGER NOT NULL DEFAULT 1,
         CHECK ((enrolment_months IS NULL) <> (enrolment_until IS NULL))
     );
     CREATE TABLE patrons (
         id              INTEGER PRIMARY KEY,
         cardnumber      TEXT NOT NULL UNIQUE,
         surname         TEXT NOT NULL,
         first_name      TEXT,
         cpf             TEXT UNIQUE,
         date_of_birth   TEXT,
         email           TEXT,
         phone           TEXT,
         address         TEXT,
         branch_id       INTEGER NOT NULL REFERENCES branches (id),
         category_id     INTEGER NOT NULL REFERENCES patron_categories (id),
         enrolled        TEXT NOT NULL,
         expires         TEXT NOT NULL,
         staff_note      TEXT,
         reading_history INTEGER NOT NULL DEFAULT 0,
         row_version     INTEGER NOT NULL DEFAULT 1
     );
     CREATE INDEX patrons_by_name ON patrons (surname, first_name);",
    // 45: circulation rules (docs/specs/circulacao/regras.md). NULL is "any"; one value
    // per kind and scope, which the unique index enforces with NULL counted as 0.
    "CREATE TABLE circulation_rules (
         id          INTEGER PRIMARY KEY,
         kind        TEXT NOT NULL,
         branch_id   INTEGER REFERENCES branches (id),
         category_id INTEGER REFERENCES patron_categories (id),
         policy_id   INTEGER REFERENCES circulation_policies (id),
         value       TEXT NOT NULL
     );
     CREATE UNIQUE INDEX circulation_rules_scope ON circulation_rules
         (kind, coalesce(branch_id, 0), coalesce(category_id, 0), coalesce(policy_id, 0));",
    // 46: each branch's calendar (docs/specs/circulacao/calendario.md). Weekdays count
    // from Sunday = 0, as in Koha; dates are ISO text. A branch takes its calendar with it.
    "CREATE TABLE calendar_weekly (
         branch_id   INTEGER NOT NULL REFERENCES branches (id) ON DELETE CASCADE,
         weekday     INTEGER NOT NULL CHECK (weekday BETWEEN 0 AND 6),
         title       TEXT NOT NULL,
         description TEXT,
         PRIMARY KEY (branch_id, weekday)
     );
     CREATE TABLE calendar_annual (
         branch_id   INTEGER NOT NULL REFERENCES branches (id) ON DELETE CASCADE,
         month       INTEGER NOT NULL CHECK (month BETWEEN 1 AND 12),
         day         INTEGER NOT NULL CHECK (day BETWEEN 1 AND 31),
         title       TEXT NOT NULL,
         description TEXT,
         PRIMARY KEY (branch_id, month, day)
     );
     CREATE TABLE calendar_dates (
         branch_id   INTEGER NOT NULL REFERENCES branches (id) ON DELETE CASCADE,
         day         TEXT NOT NULL,
         open        INTEGER NOT NULL CHECK (open IN (0, 1)),
         title       TEXT NOT NULL,
         description TEXT,
         PRIMARY KEY (branch_id, day)
     );",
    // 47: current loans (docs/specs/circulacao/emprestimo.md). One per Item; times are
    // RFC 9557 text in the library's time zone, as the command carried them.
    "CREATE TABLE loans (
         id        INTEGER PRIMARY KEY,
         item_id   INTEGER NOT NULL UNIQUE REFERENCES items (id),
         patron_id INTEGER NOT NULL REFERENCES patrons (id),
         branch_id INTEGER NOT NULL REFERENCES branches (id),
         issued_at TEXT NOT NULL,
         due       TEXT NOT NULL,
         on_site   INTEGER NOT NULL CHECK (on_site IN (0, 1))
     );
     CREATE INDEX loans_by_patron ON loans (patron_id);",
    // 48: ended loans and transfers between branches (docs/specs/circulacao/devolucao.md).
    // An ended loan keeps its patron only when the patron's reading history is kept.
    "CREATE TABLE returned_loans (
         id          INTEGER PRIMARY KEY,
         item_id     INTEGER NOT NULL REFERENCES items (id),
         patron_id   INTEGER REFERENCES patrons (id),
         branch_id   INTEGER NOT NULL REFERENCES branches (id),
         issued_at   TEXT NOT NULL,
         due         TEXT NOT NULL,
         on_site     INTEGER NOT NULL CHECK (on_site IN (0, 1)),
         returned_at TEXT NOT NULL,
         return_branch_id INTEGER NOT NULL REFERENCES branches (id)
     );
     CREATE INDEX returned_loans_by_patron ON returned_loans (patron_id)
         WHERE patron_id IS NOT NULL;
     CREATE TABLE transfers (
         id          INTEGER PRIMARY KEY,
         item_id     INTEGER NOT NULL REFERENCES items (id),
         from_branch INTEGER NOT NULL REFERENCES branches (id),
         to_branch   INTEGER NOT NULL REFERENCES branches (id),
         reason      TEXT NOT NULL,
         sent_at     TEXT NOT NULL,
         arrived_at  TEXT
     );
     CREATE UNIQUE INDEX transfers_open ON transfers (item_id) WHERE arrived_at IS NULL;",
    // 49: renewals (docs/specs/circulacao/renovacao.md); an ended loan keeps its count.
    "ALTER TABLE loans ADD COLUMN renewals INTEGER NOT NULL DEFAULT 0;
     ALTER TABLE loans ADD COLUMN renewed_at TEXT;
     ALTER TABLE returned_loans ADD COLUMN renewals INTEGER NOT NULL DEFAULT 0;",
    // 50: restrictions on a patron (docs/specs/circulacao/suspensao.md). A NULL `expires`
    // lasts until lifted; a patron has at most one suspension, which a later one extends.
    "CREATE TABLE patron_restrictions (
         id         INTEGER PRIMARY KEY,
         patron_id  INTEGER NOT NULL REFERENCES patrons (id),
         kind       TEXT NOT NULL CHECK (kind IN ('SUSPENSION')),
         expires    TEXT,
         created_at TEXT NOT NULL
     );
     CREATE UNIQUE INDEX patron_restrictions_one_suspension
         ON patron_restrictions (patron_id) WHERE kind = 'SUSPENSION';",
    // 51: a patron's account (docs/specs/circulacao/conta.md). Debits and credits share the
    // table, as in Koha: a line is one or the other, amounts in cents, credits negative.
    // The types are checked by the code, so a new one needs no table rebuild.
    "CREATE TABLE account_lines (
         id          INTEGER PRIMARY KEY,
         patron_id   INTEGER NOT NULL REFERENCES patrons (id),
         debit_type  TEXT,
         credit_type TEXT,
         status      TEXT,
         amount      INTEGER NOT NULL,
         outstanding INTEGER NOT NULL,
         item_id     INTEGER,
         loan_id     INTEGER,
         branch_id   INTEGER,
         staff_id    INTEGER,
         created_at  TEXT NOT NULL,
         updated_at  TEXT NOT NULL,
         CHECK ((debit_type IS NULL) <> (credit_type IS NULL))
     );
     CREATE INDEX account_lines_by_patron ON account_lines (patron_id);
     CREATE TABLE account_offsets (
         id         INTEGER PRIMARY KEY,
         credit_id  INTEGER REFERENCES account_lines (id),
         debit_id   INTEGER REFERENCES account_lines (id),
         kind       TEXT NOT NULL,
         amount     INTEGER NOT NULL,
         created_at TEXT NOT NULL
     );",
    // 52: holds (docs/specs/circulacao/reservas.md). `priority` is the place in the
    // Instance's queue; a hold that ends moves to `ended_holds`, without its patron unless
    // the patron keeps a reading history.
    "CREATE TABLE holds (
         id          INTEGER PRIMARY KEY,
         patron_id   INTEGER NOT NULL REFERENCES patrons (id),
         instance_id INTEGER NOT NULL REFERENCES instances (id),
         item_id     INTEGER REFERENCES items (id),
         pickup_id   INTEGER NOT NULL REFERENCES branches (id),
         priority    INTEGER NOT NULL,
         placed_on   TEXT NOT NULL,
         created_at  TEXT NOT NULL
     );
     CREATE INDEX holds_by_instance ON holds (instance_id, priority);
     CREATE INDEX holds_by_patron ON holds (patron_id);
     CREATE TABLE ended_holds (
         id          INTEGER PRIMARY KEY,
         patron_id   INTEGER REFERENCES patrons (id),
         instance_id INTEGER NOT NULL REFERENCES instances (id),
         item_id     INTEGER REFERENCES items (id),
         pickup_id   INTEGER NOT NULL REFERENCES branches (id),
         placed_on   TEXT NOT NULL,
         ended_at    TEXT NOT NULL,
         outcome     TEXT NOT NULL CHECK (outcome IN ('CANCELLED'))
     );",
    // 53: a hold set aside for its patron (docs/specs/circulacao/reservas.md, part b).
    // `found` is `W` waiting at the pickup branch or `T` in transit to it, with priority 0;
    // `item_level` keeps whether the patron asked for the Item, since a record hold gets its
    // Item when it is set aside. A filled hold ends as `FILLED`, so `ended_holds` is rebuilt
    // with the wider check.
    "ALTER TABLE holds ADD COLUMN item_level INTEGER NOT NULL DEFAULT 0
         CHECK (item_level IN (0, 1));
     UPDATE holds SET item_level = 1 WHERE item_id IS NOT NULL;
     ALTER TABLE holds ADD COLUMN found TEXT CHECK (found IN ('W', 'T'));
     ALTER TABLE holds ADD COLUMN waiting_since TEXT;
     ALTER TABLE holds ADD COLUMN expires_on TEXT;
     CREATE TABLE ended_holds_new (
         id          INTEGER PRIMARY KEY,
         patron_id   INTEGER REFERENCES patrons (id),
         instance_id INTEGER NOT NULL REFERENCES instances (id),
         item_id     INTEGER REFERENCES items (id),
         pickup_id   INTEGER NOT NULL REFERENCES branches (id),
         placed_on   TEXT NOT NULL,
         ended_at    TEXT NOT NULL,
         outcome     TEXT NOT NULL CHECK (outcome IN ('CANCELLED', 'FILLED'))
     );
     INSERT INTO ended_holds_new SELECT * FROM ended_holds;
     DROP TABLE ended_holds;
     ALTER TABLE ended_holds_new RENAME TO ended_holds;",
    // 54: overdue notice rules and the queue of notices (docs/specs/circulacao/avisos.md).
    // A NULL branch is any branch. A notice goes out once per patron, kind, level, branch,
    // transport and day, which the unique index enforces for the hourly run.
    "CREATE TABLE overdue_notice_rules (
         id          INTEGER PRIMARY KEY,
         branch_id   INTEGER REFERENCES branches (id),
         category_id INTEGER NOT NULL REFERENCES patron_categories (id),
         level       INTEGER NOT NULL CHECK (level BETWEEN 1 AND 3),
         delay       INTEGER NOT NULL CHECK (delay > 0),
         email       INTEGER NOT NULL CHECK (email IN (0, 1)),
         print       INTEGER NOT NULL CHECK (print IN (0, 1))
     );
     CREATE UNIQUE INDEX overdue_notice_rules_scope
         ON overdue_notice_rules (coalesce(branch_id, 0), category_id, level);
     CREATE TABLE notices (
         id         INTEGER PRIMARY KEY,
         patron_id  INTEGER NOT NULL REFERENCES patrons (id),
         kind       TEXT NOT NULL CHECK (kind IN ('OVERDUE')),
         level      INTEGER CHECK (level BETWEEN 1 AND 3),
         branch_id  INTEGER NOT NULL REFERENCES branches (id),
         transport  TEXT NOT NULL CHECK (transport IN ('email', 'print')),
         to_address TEXT,
         status     TEXT NOT NULL DEFAULT 'pending'
                    CHECK (status IN ('pending', 'sent', 'failed')),
         day        TEXT NOT NULL,
         created_at TEXT NOT NULL
     );
     CREATE UNIQUE INDEX notices_once
         ON notices (patron_id, kind, coalesce(level, 0), branch_id, transport, day);
     CREATE TABLE notice_items (
         notice_id INTEGER NOT NULL REFERENCES notices (id),
         item_id   INTEGER NOT NULL REFERENCES items (id),
         due       TEXT NOT NULL,
         PRIMARY KEY (notice_id, item_id)
     );",
    // 55: restrictions for overdue notices (docs/specs/circulacao/avisos.md, part b). A
    // patron has at most one restriction of each kind, so the table is rebuilt with the
    // wider check; `daily_runs` keeps the day a once-a-day run last went through.
    "ALTER TABLE overdue_notice_rules ADD COLUMN restricts INTEGER NOT NULL DEFAULT 0
         CHECK (restricts IN (0, 1));
     CREATE TABLE patron_restrictions_new (
         id         INTEGER PRIMARY KEY,
         patron_id  INTEGER NOT NULL REFERENCES patrons (id),
         kind       TEXT NOT NULL CHECK (kind IN ('SUSPENSION', 'OVERDUES')),
         expires    TEXT,
         created_at TEXT NOT NULL
     );
     INSERT INTO patron_restrictions_new SELECT * FROM patron_restrictions;
     DROP TABLE patron_restrictions;
     ALTER TABLE patron_restrictions_new RENAME TO patron_restrictions;
     CREATE UNIQUE INDEX patron_restrictions_one_per_kind
         ON patron_restrictions (patron_id, kind);
     CREATE TABLE daily_runs (
         name TEXT PRIMARY KEY,
         day  TEXT NOT NULL
     );",
    // 56: when the sender sent a notice, or gave up on it (docs/specs/circulacao/avisos.md,
    // part c).
    "ALTER TABLE notices ADD COLUMN sent_at TEXT;
     CREATE INDEX notices_pending ON notices (id) WHERE status = 'pending';",
    // 57: the notice of a hold waiting to be picked up (docs/specs/circulacao/avisos.md,
    // part d), by patron category. `notices` takes the new kind and the hold's last day,
    // and only overdue notices are kept to one a day; an Item of a hold notice has no due
    // date, so both tables are rebuilt.
    "CREATE TABLE hold_notice_preferences (
         category_id INTEGER PRIMARY KEY REFERENCES patron_categories (id),
         email       INTEGER NOT NULL CHECK (email IN (0, 1)),
         print       INTEGER NOT NULL CHECK (print IN (0, 1))
     );
     CREATE TABLE notices_new (
         id         INTEGER PRIMARY KEY,
         patron_id  INTEGER NOT NULL REFERENCES patrons (id),
         kind       TEXT NOT NULL CHECK (kind IN ('OVERDUE', 'HOLD')),
         level      INTEGER CHECK (level BETWEEN 1 AND 3),
         branch_id  INTEGER NOT NULL REFERENCES branches (id),
         transport  TEXT NOT NULL CHECK (transport IN ('email', 'print')),
         to_address TEXT,
         status     TEXT NOT NULL DEFAULT 'pending'
                    CHECK (status IN ('pending', 'sent', 'failed')),
         day        TEXT NOT NULL,
         created_at TEXT NOT NULL,
         sent_at    TEXT,
         hold_until TEXT
     );
     INSERT INTO notices_new (id, patron_id, kind, level, branch_id, transport, to_address,
                              status, day, created_at, sent_at)
         SELECT id, patron_id, kind, level, branch_id, transport, to_address, status, day,
                created_at, sent_at
         FROM notices;
     CREATE TABLE notice_items_new (
         notice_id INTEGER NOT NULL REFERENCES notices (id),
         item_id   INTEGER NOT NULL REFERENCES items (id),
         due       TEXT,
         PRIMARY KEY (notice_id, item_id)
     );
     INSERT INTO notice_items_new SELECT * FROM notice_items;
     DROP TABLE notice_items;
     DROP TABLE notices;
     ALTER TABLE notices_new RENAME TO notices;
     ALTER TABLE notice_items_new RENAME TO notice_items;
     CREATE UNIQUE INDEX notices_once
         ON notices (patron_id, kind, coalesce(level, 0), branch_id, transport, day)
         WHERE kind = 'OVERDUE';
     CREATE INDEX notices_pending ON notices (id) WHERE status = 'pending';",
    // 58: the texts of the notices the library wrote in place of the built-in ones
    // (docs/specs/circulacao/avisos.md, part g).
    "CREATE TABLE notice_texts (
         kind    TEXT PRIMARY KEY
                 CHECK (kind IN ('OVERDUE1', 'OVERDUE2', 'OVERDUE3', 'HOLD')),
         subject TEXT NOT NULL,
         body    TEXT NOT NULL
     );",
    // 59: the notice board of the homes (docs/specs/interface/mural.md).
    "CREATE TABLE board_notes (
         id          INTEGER PRIMARY KEY,
         kind        TEXT NOT NULL
                     CHECK (kind IN ('staff', 'institutional', 'personal', 'urgent')),
         audience    TEXT NOT NULL CHECK (audience IN ('staff', 'public')),
         text        TEXT NOT NULL,
         author_id   INTEGER NOT NULL REFERENCES staff_accounts (id),
         day         TEXT NOT NULL,
         row_version INTEGER NOT NULL DEFAULT 1
     ) STRICT;",
    // 60: the patron's OPAC password, Argon2id (opac/conta-do-leitor.md); NULL: no access.
    "ALTER TABLE patrons ADD COLUMN password_hash TEXT;",
    // 61: keep installation-local identifiers separate across source catalogs.
    include_str!("import_provenance.sql"),
    // 62: offline institutional repository.
    include_str!("repository/schema.sql"),
    // 63: the OAI-PMH datestamp of each Instance; 64: the Koha keys of migrated patrons.
    include_str!("oai_datestamps.sql"),
    include_str!("koha_keys.sql"),
];

/// The migration that creates `match_keys`.
const MATCH_KEYS_VERSION: i64 = 5;

/// The migration that creates `instance_notes`.
const NOTES_VERSION: i64 = 6;

/// The migration that adds `works.audience`.
const AUDIENCE_VERSION: i64 = 7;

/// The migration that adds the listing `sort_key` of Works and Hubs.
const SORT_KEY_VERSION: i64 = 8;
/// Migration that adds the registries of places and the edge table.
const PLACES_VERSION: i64 = 14;
/// Migration that drops the codes of the core; it needs foreign keys off.
const NO_CODES_VERSION: i64 = 15;
/// Migration that turns item types into circulation policies; it needs foreign keys off.
const POLICIES_VERSION: i64 = 16;
/// Migration that fills the titles other than the preferred one.
const TITLES_VERSION: i64 = 18;
/// Migration that fills the typed notes, custodial history and frequency.
const TYPED_NOTES_VERSION: i64 = 19;
/// Migration that fills the other provision activities and every place and publisher.
const PROVISION_VERSION: i64 = 20;
/// Migration that fills the series and related works.
const RELATIONS_VERSION: i64 = 21;
/// Migration that fills coverage, languages of the parts, scripts and electronic locations.
const CONTENT_VERSION: i64 = 22;
/// Migration that splits the physical description out of the extent.
const PHYSICAL_VERSION: i64 = 23;
/// Migration that fills the administrative metadata.
const ADMIN_VERSION: i64 = 24;
/// Migration that fills the coded publication of `008`.
const FIXED_VERSION: i64 = 25;
/// Migration that fills the statement of responsibility.
const RESPONSIBILITY_VERSION: i64 = 26;
/// Migration that keeps what each subdivision of a subject names.
const SUBDIVISIONS_VERSION: i64 = 27;
/// Migration that keeps the parts of an agent's heading.
const NAME_PARTS_VERSION: i64 = 28;
/// Migration that keeps the identifiers and the relator term of a contribution.
const AGENT_IDENTIFIERS_VERSION: i64 = 29;
/// Migration that tells the agents named as subjects apart and keeps their parts and links.
const SUBJECT_AGENTS_VERSION: i64 = 30;
/// Migration that reads the description conventions of Leader/18.
const LEADER_CONVENTIONS_VERSION: i64 = 31;
/// Migration that reads the classifications.
const CLASSIFICATIONS_VERSION: i64 = 32;
/// Migration that reads the positions of a book in `008/18-34`.
const BOOKS_VERSION: i64 = 33;
/// Migration that reads the time periods (`045`) and the national numbers (`016`).
const PERIODS_VERSION: i64 = 34;
/// Migration that reads the forms of the Work (`380`).
const FORMS_VERSION: i64 = 35;
/// Migration that drops the category of material (bibframe-completo.md, item 14b).
#[cfg(test)]
const NO_CATEGORY_VERSION: i64 = 36;

/// The migration that keeps when each command ran and which entities it touched; only the
/// tests that rewind the schema need its number.
#[cfg(test)]
const HISTORY_VERSION: i64 = 37;
/// Migration that gives agents a version; only the tests that rewind the schema need it.
#[cfg(test)]
const AGENT_VERSION: i64 = 39;
/// Migration that gives subjects a version; only the tests that rewind the schema need it.
#[cfg(test)]
const SUBJECT_VERSION: i64 = 40;
/// Migration that indexes the `audit_log` for the work lists; only the tests that rewind
/// the schema need it.
#[cfg(test)]
const WORK_LISTS_VERSION: i64 = 41;
/// Migration that lets an Instance be hidden from the OPAC.
const OPAC_HIDDEN_VERSION: i64 = 38;
/// Migration that drops `extra`.
const DROP_EXTRA_VERSION: i64 = 13;
/// Migration that adds the classes, and fills them with content, media, carrier and issuance.
const CLASSES_VERSION: i64 = 12;

/// How long a connection waits on a lock before failing. With a single writer this only
/// covers checkpoints and external tools; it is not part of normal operation.
const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

/// Opens the writable connection, sets the PRAGMAs and applies pending migrations.
pub(crate) fn open_writer(path: &Path) -> Result<Connection, StoreError> {
    let mut connection = Connection::open(path)?;
    connection.busy_timeout(BUSY_TIMEOUT)?;
    connection.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA synchronous = NORMAL;
         PRAGMA foreign_keys = ON;
         PRAGMA temp_store = MEMORY;",
    )?;
    migrate(&mut connection)?;
    Ok(connection)
}

/// Opens a read-only connection for the pool.
pub(crate) fn open_reader(path: &Path) -> Result<Connection, StoreError> {
    let connection = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    connection.busy_timeout(BUSY_TIMEOUT)?;
    connection.execute_batch("PRAGMA foreign_keys = ON; PRAGMA temp_store = MEMORY;")?;
    Ok(connection)
}

/// Newest schema version this build knows.
pub(crate) fn latest_version() -> i64 {
    i64::try_from(MIGRATIONS.len()).unwrap_or(i64::MAX)
}

fn migrate(connection: &mut Connection) -> Result<(), StoreError> {
    let current: i64 = connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    let supported = latest_version();
    if current > supported {
        return Err(StoreError::SchemaTooNew {
            found: current,
            supported,
        });
    }
    // Foreign keys can only be switched outside a transaction.
    if current < 60 {
        migrate_range(connection, current, 60, current < POLICIES_VERSION)?;
    }
    migrate_range(connection, current.max(60), supported, current < 61)
}

fn migrate_range(
    connection: &mut Connection,
    current: i64,
    target: i64,
    rebuilds: bool,
) -> Result<(), StoreError> {
    if rebuilds {
        connection.execute_batch("PRAGMA foreign_keys = OFF;")?;
    }
    let migrated = apply_migrations(connection, current, target);
    if rebuilds {
        connection.execute_batch("PRAGMA foreign_keys = ON;")?;
    }
    migrated
}

fn apply_migrations(
    connection: &mut Connection,
    current: i64,
    target: i64,
) -> Result<(), StoreError> {
    let tx = connection.transaction()?;
    for (version, sql) in (1_i64..).zip(MIGRATIONS) {
        if version > current && version <= target {
            if version == DROP_EXTRA_VERSION {
                check_extra_is_empty(&tx)?;
            }
            tx.execute_batch(sql)?;
            if version == MATCH_KEYS_VERSION {
                backfill_match_keys(&tx)?;
            }
            if version == NOTES_VERSION {
                backfill_notes(&tx)?;
            }
            if version == AUDIENCE_VERSION {
                backfill_audience(&tx)?;
            }
            if version == SORT_KEY_VERSION {
                backfill_sort_keys(&tx)?;
            }
            if version == CLASSES_VERSION {
                backfill_descriptions(&tx)?;
            }
            if version == PLACES_VERSION {
                crate::places::backfill(&tx)?;
            }
            if version == TITLES_VERSION {
                backfill_from_source(&tx, source_titles)?;
            }
            if version == TYPED_NOTES_VERSION {
                backfill_from_source(&tx, source_typed_notes)?;
            }
            if version == PROVISION_VERSION {
                backfill_from_source(&tx, source_provision)?;
                backfill_publication(&tx)?;
            }
            if version == RELATIONS_VERSION {
                backfill_from_source(&tx, source_relations)?;
            }
            if version == CONTENT_VERSION {
                backfill_from_source(&tx, source_content)?;
            }
            if version == PHYSICAL_VERSION {
                backfill_physical(&tx)?;
            }
            if version == ADMIN_VERSION {
                backfill_from_source(&tx, source_admin)?;
            }
            if version == FIXED_VERSION {
                backfill_from_source(&tx, source_fixed)?;
            }
            if version == RESPONSIBILITY_VERSION {
                backfill_from_source(&tx, source_responsibility)?;
            }
            if version == SUBDIVISIONS_VERSION {
                backfill_subdivisions(&tx)?;
            }
            if version == NAME_PARTS_VERSION {
                backfill_name_parts(&tx)?;
            }
            if version == AGENT_IDENTIFIERS_VERSION {
                backfill_agent_identifiers(&tx)?;
            }
            if version == SUBJECT_AGENTS_VERSION {
                backfill_subject_agents(&tx)?;
            }
            if version == LEADER_CONVENTIONS_VERSION {
                backfill_leader_conventions(&tx)?;
            }
            if version == CLASSIFICATIONS_VERSION {
                backfill_from_source(&tx, source_classification)?;
            }
            if version == BOOKS_VERSION {
                backfill_from_source(&tx, source_books)?;
            }
            if version == PERIODS_VERSION {
                backfill_from_source(&tx, source_periods_and_numbers)?;
            }
            if version == FORMS_VERSION {
                backfill_from_source(&tx, source_forms)?;
            }
            if version == OPAC_HIDDEN_VERSION {
                backfill_opac_hidden(&tx)?;
            }
            if version == 61 {
                crate::catalog::backfill_item_issues(&tx)?;
            }
            if version == NO_CODES_VERSION || version == POLICIES_VERSION || version == 61 {
                check_foreign_keys(&tx, version)?;
            }
            tx.pragma_update(None, "user_version", version)?;
        }
    }
    tx.commit()?;
    Ok(())
}

/// Hides from the OPAC every stored Instance whose source record hides it.
fn backfill_opac_hidden(connection: &Connection) -> Result<(), StoreError> {
    let mut rows = connection.prepare(
        "SELECT i.id, s.record FROM instances i JOIN source_records s ON s.instance_id = i.id",
    )?;
    let mut rows = rows.query([])?;
    let mut hide = connection.prepare("UPDATE instances SET opac_hidden = 1 WHERE id = ?1")?;
    while let Some(row) = rows.next()? {
        let id: i64 = row.get(0)?;
        let source: Vec<u8> = row.get(1)?;
        if source_opac_hidden(&source) {
            hide.execute(params![id])?;
        }
    }
    Ok(())
}

/// Computes the match keys of every record already stored when `match_keys` is created.
fn backfill_match_keys(connection: &Connection) -> Result<(), StoreError> {
    let mut insert = connection
        .prepare("INSERT OR IGNORE INTO match_keys (rule, key, instance_id) VALUES (?1, ?2, ?3)")?;
    let mut identifiers =
        connection.prepare("SELECT instance_id, kind, value FROM instance_identifiers")?;
    let mut rows = identifiers.query([])?;
    while let Some(row) = rows.next()? {
        let kind: u8 = row.get(1)?;
        let value: String = row.get(2)?;
        let key = IdentifierKind::from_code(kind).and_then(|kind| identifier_key(kind, &value));
        if let Some(key) = key {
            insert.execute(params![key.rule.code(), key.key, row.get::<_, i64>(0)?])?;
        }
    }
    let mut sources = connection.prepare("SELECT instance_id, record FROM source_records")?;
    let mut rows = sources.query([])?;
    while let Some(row) = rows.next()? {
        let record: Vec<u8> = row.get(1)?;
        if let Some(key) = koha_biblionumber_key(&record) {
            insert.execute(params![key.rule.code(), key.key, row.get::<_, i64>(0)?])?;
        }
    }
    Ok(())
}

/// Reads the notes of every record already stored when `instance_notes` is created. Without
/// them, the export would see the notes of the original as removed and drop them.
fn backfill_notes(connection: &Connection) -> Result<(), StoreError> {
    let mut sources = connection.prepare("SELECT instance_id, record FROM source_records")?;
    let mut rows = sources.query([])?;
    while let Some(row) = rows.next()? {
        let record: Vec<u8> = row.get(1)?;
        insert_notes(connection, row.get(0)?, &source_notes(&record))?;
    }
    Ok(())
}

/// Reads the target audience of every Work already stored when `works.audience` is
/// added, from the source record of its first Instance, for the same reason as the notes.
fn backfill_audience(connection: &Connection) -> Result<(), StoreError> {
    let mut sources = connection.prepare(
        "SELECT instances.work_id, source_records.record FROM source_records
         JOIN instances ON instances.id = source_records.instance_id
         ORDER BY instances.id",
    )?;
    let mut update =
        connection.prepare("UPDATE works SET audience = ?2 WHERE id = ?1 AND audience IS NULL")?;
    let mut rows = sources.query([])?;
    while let Some(row) = rows.next()? {
        let record: Vec<u8> = row.get(1)?;
        if let Some(audience) = source_audience(&record) {
            update.execute(params![row.get::<_, i64>(0)?, audience])?;
        }
    }
    Ok(())
}

/// After a rebuild with foreign keys off, every reference must still hold.
fn check_foreign_keys(connection: &Connection, version: i64) -> Result<(), StoreError> {
    let broken: i64 =
        connection.query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |row| {
            row.get(0)
        })?;
    if broken > 0 {
        return Err(StoreError::Sqlite(format!(
            "{broken} rows reference missing rows after migration {version}"
        )));
    }
    Ok(())
}

/// No version of Elzevir wrote `extra`, so a value in it came from outside: the migration
/// stops rather than lose it.
fn check_extra_is_empty(connection: &Connection) -> Result<(), StoreError> {
    for table in ["hubs", "works", "instances", "items"] {
        // Table names come from the constant list above, never from data.
        let filled: i64 = connection.query_row(
            &format!("SELECT count(*) FROM {table} WHERE extra <> '{{}}'"),
            [],
            |row| row.get(0),
        )?;
        if filled > 0 {
            return Err(StoreError::Sqlite(format!(
                "{filled} rows of {table} hold data in `extra`; move it to statements first"
            )));
        }
    }
    Ok(())
}

/// Describes every Work and Instance already stored (bibframe-completo.md, item 31): from
/// the source record as the import now reads it, or else from the category of material.
fn backfill_descriptions(connection: &Connection) -> Result<(), StoreError> {
    let mut works = connection.prepare(
        "SELECT w.id, w.material_type, i.id, i.material_type, s.record
         FROM works w
         LEFT JOIN instances i ON i.work_id = w.id
         LEFT JOIN source_records s ON s.instance_id = i.id
         ORDER BY w.id, s.record IS NULL, i.id",
    )?;
    let mut rows = works.query([])?;
    let mut described_work = None;
    while let Some(row) = rows.next()? {
        let work: i64 = row.get(0)?;
        let instance: Option<i64> = row.get(2)?;
        let source: Option<Vec<u8>> = row.get(4)?;
        let from_source = source.as_deref().and_then(source_description);
        let (work_description, instance_description) = match from_source {
            Some(both) => both,
            None => legacy_description(row.get(1)?, row.get(3)?)?,
        };
        // The first row of a Work is the one with a source record, when it has one.
        if described_work != Some(work) {
            described_work = Some(work);
            describe(connection, EntityKind::Work, work, &work_description)?;
        }
        if let Some(instance) = instance {
            describe(
                connection,
                EntityKind::Instance,
                instance,
                &instance_description,
            )?;
        }
    }
    Ok(())
}

/// Gives every stored record all the places and publishers of its source record. A value
/// changed since the import (not the first `$a` or `$b` any more) is left as it is.
fn backfill_publication(connection: &Connection) -> Result<(), StoreError> {
    let mut rows = connection.prepare(
        "SELECT i.id, s.record FROM instances i JOIN source_records s ON s.instance_id = i.id",
    )?;
    let mut rows = rows.query([])?;
    let mut place =
        connection.prepare("UPDATE instances SET place = ?2 WHERE id = ?1 AND place IS ?3")?;
    let mut publisher = connection
        .prepare("UPDATE instances SET publisher = ?2 WHERE id = ?1 AND publisher IS ?3")?;
    while let Some(row) = rows.next()? {
        let id: i64 = row.get(0)?;
        let source: Vec<u8> = row.get(1)?;
        let Some([(old_place, new_place), (old_publisher, new_publisher)]) =
            source_publication(&source)
        else {
            continue;
        };
        place.execute(params![id, new_place, old_place])?;
        publisher.execute(params![id, new_publisher, old_publisher])?;
    }
    Ok(())
}

/// Moves the physical details, dimensions and accompanying material of every stored record
/// out of the extent into statements. An extent edited since the import is left as the
/// cataloguer wrote it, and its record gets no statements that would repeat it.
fn backfill_physical(connection: &Connection) -> Result<(), StoreError> {
    let mut rows = connection.prepare(
        "SELECT i.id, s.record FROM instances i JOIN source_records s ON s.instance_id = i.id",
    )?;
    let mut rows = rows.query([])?;
    let mut extent =
        connection.prepare("UPDATE instances SET extent = ?2 WHERE id = ?1 AND extent IS ?3")?;
    while let Some(row) = rows.next()? {
        let id: i64 = row.get(0)?;
        let source: Vec<u8> = row.get(1)?;
        let Some((old, new)) = source_extent(&source) else {
            continue;
        };
        let unedited = old == new || extent.execute(params![id, new, old])? == 1;
        if unedited && let Some((_, of_instance)) = source_physical(&source) {
            add_missing(connection, EntityKind::Instance, id, of_instance)?;
        }
    }
    Ok(())
}

/// Gives every stored agent without name parts the parts the source records of its Works
/// give it, for the same kind and heading.
fn backfill_name_parts(connection: &Connection) -> Result<(), StoreError> {
    let mut sources = connection.prepare("SELECT record FROM source_records")?;
    let mut rows = sources.query([])?;
    while let Some(row) = rows.next()? {
        let source: Vec<u8> = row.get(0)?;
        for contribution in source_contributions(&source).unwrap_or_default() {
            set_name_parts(connection, &contribution)?;
        }
    }
    Ok(())
}

/// Gives every stored contribution the identifiers and the relator term its source record
/// writes, where the Work still has the contribution as the record gives it.
fn backfill_agent_identifiers(connection: &Connection) -> Result<(), StoreError> {
    let mut sources = connection.prepare(
        "SELECT i.work_id, s.record FROM instances i
         JOIN source_records s ON s.instance_id = i.id
         WHERE i.id = (SELECT min(id) FROM instances WHERE work_id = i.work_id
                       AND id IN (SELECT instance_id FROM source_records))",
    )?;
    let mut same = connection.prepare(
        "SELECT count(*) FROM contributions c JOIN agents a ON a.id = c.agent_id
         WHERE c.work_id = ?1 AND c.position = ?2 AND a.kind = ?3 AND a.heading = ?4",
    )?;
    let mut term = connection
        .prepare("UPDATE contributions SET role_term = ?3 WHERE work_id = ?1 AND position = ?2")?;
    let mut rows = sources.query([])?;
    while let Some(row) = rows.next()? {
        let work: i64 = row.get(0)?;
        let source: Vec<u8> = row.get(1)?;
        let contributions = source_contributions(&source).unwrap_or_default();
        for (contribution, position) in contributions.iter().zip(0_i64..) {
            let kept: i64 = same.query_row(
                params![
                    work,
                    position,
                    contribution.kind.code(),
                    contribution.heading
                ],
                |row| row.get(0),
            )?;
            if kept == 0 {
                continue;
            }
            insert_contribution_identifiers(connection, work, position, contribution)?;
            if let Some(role_term) = &contribution.role_term {
                term.execute(params![work, position, role_term])?;
            }
        }
    }
    Ok(())
}

/// Gives the subjects of every stored Work what the source record of its first Instance
/// says of them: the kind of agent a name heading is, the parts of the name and the links.
/// A subject is changed only where the Work still has it at the place and with the heading
/// the record gives it, and with its kind or, for a name stored before the kinds were told
/// apart, as a person.
fn backfill_subject_agents(connection: &Connection) -> Result<(), StoreError> {
    let mut sources = connection.prepare(
        "SELECT i.work_id, s.record FROM instances i
         JOIN source_records s ON s.instance_id = i.id
         WHERE i.id = (SELECT min(id) FROM instances WHERE work_id = i.work_id
                       AND id IN (SELECT instance_id FROM source_records))",
    )?;
    let mut stored = connection.prepare(
        "SELECT s.kind, s.thesaurus FROM work_subjects ws JOIN subjects s ON s.id = ws.subject_id
         WHERE ws.work_id = ?1 AND ws.position = ?2 AND s.heading = ?3",
    )?;
    let mut repoint = connection
        .prepare("UPDATE work_subjects SET subject_id = ?3 WHERE work_id = ?1 AND position = ?2")?;
    let mut rows = sources.query([])?;
    while let Some(row) = rows.next()? {
        let work: i64 = row.get(0)?;
        let source: Vec<u8> = row.get(1)?;
        let subjects = source_subjects(&source).unwrap_or_default();
        for (subject, position) in subjects.iter().zip(0_i64..) {
            let present: Option<(u8, Option<i64>)> = stored
                .query_row(params![work, position, subject.heading], |row| {
                    Ok((row.get(0)?, row.get(1)?))
                })
                .optional()?;
            let Some((kind, thesaurus)) = present else {
                continue;
            };
            let person = SubjectKind::Person.code();
            if kind != subject.kind.code() && !(kind == person && subject.kind.agent().is_some()) {
                continue;
            }
            let id = subject_id(connection, subject, thesaurus)?;
            repoint.execute(params![work, position, id])?;
            insert_subject_identifiers(connection, work, position, subject)?;
        }
    }
    Ok(())
}

/// A subject of a Work as stored: position, kind, heading, thesaurus id and its code.
type StoredSubject = (i64, u8, String, Option<i64>, Option<String>);

/// Gives the subjects of every stored Work what their subdivisions name, from the source
/// record of its first Instance. A subject is changed only where the Work still has it as
/// the source record gives it (kind, heading and thesaurus), so an edited one stays as is.
fn backfill_subdivisions(connection: &Connection) -> Result<(), StoreError> {
    let mut sources = connection.prepare(
        "SELECT i.work_id, s.record FROM instances i
         JOIN source_records s ON s.instance_id = i.id
         WHERE i.id = (SELECT min(id) FROM instances WHERE work_id = i.work_id
                       AND id IN (SELECT instance_id FROM source_records))",
    )?;
    let mut current = connection.prepare(
        "SELECT ws.position, s.kind, s.heading, s.thesaurus, t.code FROM work_subjects ws
         JOIN subjects s ON s.id = ws.subject_id
         LEFT JOIN terms t ON t.id = s.thesaurus
         WHERE ws.work_id = ?1 AND s.subdivisions = '' ORDER BY ws.position",
    )?;
    let mut repoint = connection
        .prepare("UPDATE work_subjects SET subject_id = ?3 WHERE work_id = ?1 AND position = ?2")?;
    let mut rows = sources.query([])?;
    while let Some(row) = rows.next()? {
        let work: i64 = row.get(0)?;
        let source: Vec<u8> = row.get(1)?;
        let Some(subjects) = source_subjects(&source) else {
            continue;
        };
        let mut present: Vec<StoredSubject> = current
            .query_map(params![work], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            })?
            .collect::<Result<_, _>>()?;
        for subject in subjects.iter().filter(|s| !s.subdivisions.is_empty()) {
            let found = present.iter().position(|(_, kind, heading, _, thesaurus)| {
                *kind == subject.kind.code()
                    && *heading == subject.heading
                    && *thesaurus == subject.thesaurus
            });
            let Some(found) = found else {
                continue;
            };
            let (position, _, _, thesaurus, _) = present.remove(found);
            let id = subject_id(connection, subject, thesaurus)?;
            repoint.execute(params![work, position, id])?;
        }
    }
    Ok(())
}

/// The statements of the Work and of the Instance that a stored source record gives.
type FromSource = fn(&[u8]) -> Option<(Vec<Statement>, Vec<Statement>)>;

/// Adds to every stored record the statements `read` finds in its source record that its
/// Work and its Instance do not have yet. A Work takes them from its first Instance with a
/// source.
fn backfill_from_source(connection: &Connection, read: FromSource) -> Result<(), StoreError> {
    let mut rows = connection.prepare(
        "SELECT i.work_id, i.id, s.record FROM instances i
         JOIN source_records s ON s.instance_id = i.id ORDER BY i.work_id, i.id",
    )?;
    let mut rows = rows.query([])?;
    let mut done_work = None;
    while let Some(row) = rows.next()? {
        let work: i64 = row.get(0)?;
        let instance: i64 = row.get(1)?;
        let source: Vec<u8> = row.get(2)?;
        let Some((of_work, of_instance)) = read(&source) else {
            continue;
        };
        if done_work != Some(work) {
            done_work = Some(work);
            add_missing(connection, EntityKind::Work, work, of_work)?;
        }
        add_missing(connection, EntityKind::Instance, instance, of_instance)?;
    }
    Ok(())
}

/// Gives every Instance with a stored source record the description conventions its
/// Leader/18 names: in its administrative metadata, after the status, or as the
/// administrative metadata the source record gives when the Instance has none.
fn backfill_leader_conventions(connection: &Connection) -> Result<(), StoreError> {
    let mut rows = connection.prepare(
        "SELECT i.id, s.record FROM instances i JOIN source_records s ON s.instance_id = i.id",
    )?;
    let mut rows = rows.query([])?;
    while let Some(row) = rows.next()? {
        let instance: i64 = row.get(0)?;
        let source: Vec<u8> = row.get(1)?;
        let Some(conventions) = source_leader_conventions(&source) else {
            continue;
        };
        let subject = EntityRef {
            entity: EntityKind::Instance,
            id: instance,
        };
        let mut description = read_description(connection, subject)?;
        let admin = description
            .statements
            .iter_mut()
            .find_map(|s| match &mut s.value {
                Value::Resource(admin) if s.property == "adminMetadata" => Some(admin),
                _ => None,
            });
        match admin {
            Some(admin) if admin.statements.contains(&conventions) => continue,
            Some(admin) => {
                let after_status = usize::from(
                    admin
                        .statements
                        .first()
                        .is_some_and(|s| s.property == "status"),
                );
                admin.statements.insert(after_status, conventions);
            }
            None => match source_admin(&source) {
                Some((_, admin)) if !admin.is_empty() => description.statements.extend(admin),
                _ => continue,
            },
        }
        describe(connection, EntityKind::Instance, instance, &description)?;
    }
    Ok(())
}

fn add_missing(
    connection: &Connection,
    entity: EntityKind,
    id: i64,
    statements: Vec<Statement>,
) -> Result<(), StoreError> {
    let subject = EntityRef { entity, id };
    let mut description = read_description(connection, subject)?;
    let before = description.statements.len();
    for statement in statements {
        if !description.statements.contains(&statement) {
            description.statements.push(statement);
        }
    }
    if description.statements.len() == before {
        return Ok(());
    }
    describe(connection, entity, id, &description)
}

/// What a stored category of material (the codes of migration 4) stood for, as migration
/// 12 described records without a source record (bibframe-completo.md, item 10l). Rows
/// written before categories existed read as books; only a digital Instance under a Work
/// of another category is `Electronic`.
fn legacy_description(
    work: Option<u8>,
    instance: Option<u8>,
) -> Result<(Description, Description), StoreError> {
    let work = work.unwrap_or(1);
    let (classes, content): (&[&str], Option<&str>) = match work {
        1 => (&["Text", "Monograph"], Some("txt")),
        2 => (&["Text", "Serial"], Some("txt")),
        3 => (&["Cartography"], Some("cri")),
        4 => (&["NotatedMusic"], Some("ntm")),
        // The category did not tell spoken word from music.
        5 => (&["Audio"], None),
        6 => (&["MovingImage"], Some("tdi")),
        7 => (&["Multimedia"], Some("cop")),
        8 => (&["StillImage"], Some("sti")),
        9 => (&["MixedMaterial"], None),
        code => {
            return Err(StoreError::Sqlite(format!(
                "material type code {code} is unknown"
            )));
        }
    };
    let work_description = Description {
        classes: classes.iter().map(|class| (*class).to_owned()).collect(),
        statements: content
            .map(|code| concept("content", "contentTypes", code))
            .into_iter()
            .collect(),
    };
    let instance_description = Description {
        classes: if instance == Some(7) {
            vec!["Electronic".to_owned()]
        } else {
            Vec::new()
        },
        statements: Vec::new(),
    };
    Ok((work_description, instance_description))
}

fn describe(
    connection: &Connection,
    entity: EntityKind,
    id: i64,
    description: &Description,
) -> Result<(), StoreError> {
    store_description(connection, EntityRef { entity, id }, description).map_err(|failed| {
        match failed {
            CommandFailed::Storage(error) => error,
            other => StoreError::Sqlite(format!("{entity:?} {id} cannot be described: {other}")),
        }
    })
}

/// Computes the listing key of every Work and Hub already stored.
fn backfill_sort_keys(connection: &Connection) -> Result<(), StoreError> {
    for (select, update) in [
        (
            "SELECT id, title FROM works",
            "UPDATE works SET sort_key = ?2 WHERE id = ?1",
        ),
        (
            "SELECT id, title FROM hubs",
            "UPDATE hubs SET sort_key = ?2 WHERE id = ?1",
        ),
    ] {
        let mut rows_statement = connection.prepare(select)?;
        let mut update = connection.prepare(update)?;
        let mut rows = rows_statement.query([])?;
        while let Some(row) = rows.next()? {
            let title: String = row.get(1)?;
            update.execute(params![row.get::<_, i64>(0)?, sort_key(&title)])?;
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "schema_import_tests.rs"]
mod import_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrations_apply_once_and_are_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("elzevir.db");
        drop(open_writer(&path).unwrap());
        let connection = open_writer(&path).unwrap();
        let version: i64 = connection
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, latest_version());
        let mode: String = connection
            .query_row("PRAGMA journal_mode", [], |row| row.get(0))
            .unwrap();
        assert_eq!(mode, "wal");
    }

    #[test]
    fn restore_preflight_accepts_legacy_and_current_images_and_refuses_future_schema() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("snapshot.sqlite");
        let original = Connection::open(&path).unwrap();
        original.execute_batch(MIGRATIONS.first().unwrap()).unwrap();
        original.pragma_update(None, "user_version", 1).unwrap();
        drop(original);
        assert_eq!(crate::inspect_snapshot(&path).unwrap().schema_version, 1);
        drop(open_writer(&path).unwrap());
        assert_eq!(
            crate::inspect_snapshot(&path).unwrap().schema_version,
            latest_version()
        );
        Connection::open(&path)
            .unwrap()
            .pragma_update(None, "user_version", latest_version() + 1)
            .unwrap();
        assert!(matches!(
            crate::inspect_snapshot(&path),
            Err(StoreError::SchemaTooNew { .. })
        ));
    }

    #[test]
    fn newer_schema_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("elzevir.db");
        let connection = open_writer(&path).unwrap();
        connection
            .pragma_update(None, "user_version", latest_version() + 1)
            .unwrap();
        drop(connection);
        assert!(matches!(
            open_writer(&path),
            Err(StoreError::SchemaTooNew { .. })
        ));
    }
}

#[cfg(test)]
mod catalog_tests {
    use rusqlite::{Connection, params};

    use super::*;

    #[test]
    fn migrations_6_and_7_read_notes_and_audience_from_the_source_record() {
        use elzevir_marc::{Field, Record, Subfield, Tag};
        let field = |tag: &[u8; 3], text: &str| Field::Data {
            tag: Tag::new(*tag),
            indicators: *b"  ",
            subfields: vec![Subfield {
                code: b'a',
                value: text.as_bytes().to_vec(),
            }],
        };
        let mut source = Vec::new();
        Record {
            leader: *b"00000nam a2200000   4500",
            fields: vec![
                field(b"245", "Dom Casmurro"),
                field(b"500", "Inclui índice."),
                field(b"521", "Ensino médio."),
            ],
        }
        .write_iso2709(&mut source)
        .unwrap();
        let (_dir, mut connection) = catalog();
        connection
            .execute_batch(include_str!("../tests/data/rewind_repository.sql"))
            .unwrap();
        connection
            .execute(
                "UPDATE source_records SET record = ?1 WHERE instance_id = 1",
                params![source],
            )
            .unwrap();
        connection
            .execute_batch(
                "DROP TABLE import_identities; DROP TABLE board_notes; DROP TABLE notice_texts; DROP TABLE hold_notice_preferences; DROP TABLE daily_runs; DROP TABLE notice_items; DROP TABLE notices; DROP TABLE overdue_notice_rules; DROP TABLE ended_holds; DROP TABLE holds; DROP TABLE account_offsets; DROP TABLE account_lines; DROP TABLE patron_restrictions; DROP TABLE transfers; DROP TABLE returned_loans; DROP TABLE loans; DROP TABLE calendar_dates; DROP TABLE calendar_annual; DROP TABLE calendar_weekly; DROP TABLE circulation_rules; DROP TABLE patrons; DROP TABLE patron_categories; DROP INDEX audit_imports; DROP TABLE saved_list_families; DROP TABLE saved_lists; DROP INDEX audit_by_actor; DROP INDEX audit_by_kind; DROP INDEX audit_entities_by_seq; DROP TABLE instance_notes; ALTER TABLE works DROP COLUMN audience;
                 DROP INDEX works_in_listing; DROP INDEX hubs_in_listing; ALTER TABLE works DROP COLUMN sort_key; ALTER TABLE hubs DROP COLUMN sort_key; ALTER TABLE audit_log DROP COLUMN actor; DROP INDEX agents_by_heading; DROP INDEX subjects_by_heading; DROP TABLE koha_type_statements; ALTER TABLE circulation_policies ADD COLUMN code TEXT; UPDATE circulation_policies SET code = COALESCE((SELECT code FROM koha_codes WHERE kind = 4 AND local_id = circulation_policies.id), 'P' || id); ALTER TABLE circulation_policies RENAME COLUMN name TO description; ALTER TABLE circulation_policies RENAME TO item_types; ALTER TABLE items RENAME COLUMN policy_id TO item_type; DELETE FROM koha_codes WHERE kind = 4; ALTER TABLE branches ADD COLUMN code TEXT; UPDATE branches SET code = COALESCE((SELECT code FROM koha_codes WHERE kind = 1 AND local_id = branches.id), 'B' || id); ALTER TABLE items ADD COLUMN location INTEGER REFERENCES terms (id); ALTER TABLE items ADD COLUMN collection INTEGER REFERENCES terms (id); INSERT OR IGNORE INTO terms (vocabulary, code, label) SELECT 3, k.code, p.name FROM sublocations p JOIN koha_codes k ON k.kind = 2 AND k.local_id = p.id; INSERT OR IGNORE INTO terms (vocabulary, code, label) SELECT 4, k.code, p.name FROM collections p JOIN koha_codes k ON k.kind = 3 AND k.local_id = p.id; UPDATE items SET location = (SELECT t.id FROM terms t JOIN koha_codes k ON k.kind = 2 AND k.code = t.code WHERE t.vocabulary = 3 AND k.local_id = items.sublocation_id), collection = (SELECT t.id FROM terms t JOIN koha_codes k ON k.kind = 3 AND k.code = t.code WHERE t.vocabulary = 4 AND k.local_id = items.collection_id); ALTER TABLE items DROP COLUMN sublocation_id; ALTER TABLE items DROP COLUMN collection_id; DROP TABLE sublocations; DROP TABLE collections; DROP TABLE koha_codes; ALTER TABLE branches DROP COLUMN address; ALTER TABLE branches DROP COLUMN phone; ALTER TABLE branches DROP COLUMN email; ALTER TABLE branches DROP COLUMN url; ALTER TABLE hubs ADD COLUMN extra TEXT NOT NULL DEFAULT '{}'; ALTER TABLE works ADD COLUMN extra TEXT NOT NULL DEFAULT '{}'; ALTER TABLE instances ADD COLUMN extra TEXT NOT NULL DEFAULT '{}'; ALTER TABLE items ADD COLUMN extra TEXT NOT NULL DEFAULT '{}'; DROP TABLE entity_classes; DROP TABLE statements; DROP TABLE lc_concepts; DROP TABLE ontology_names; DROP TABLE subject_identifiers; ALTER TABLE subjects DROP COLUMN name_parts; DROP TABLE contribution_identifiers; ALTER TABLE contributions DROP COLUMN role_term; ALTER TABLE agents DROP COLUMN name_parts; DROP INDEX subjects_identity; ALTER TABLE subjects DROP COLUMN subdivisions; CREATE UNIQUE INDEX subjects_identity ON subjects (kind, heading, coalesce(thesaurus, 0)); ALTER TABLE works ADD COLUMN material_type INTEGER; ALTER TABLE instances ADD COLUMN material_type INTEGER; ALTER TABLE subjects DROP COLUMN row_version; ALTER TABLE agents DROP COLUMN row_version; ALTER TABLE instances DROP COLUMN opac_hidden; DROP TABLE audit_entities; ALTER TABLE audit_log DROP COLUMN at; PRAGMA user_version = 5;",
            )
            .unwrap();
        migrate(&mut connection).unwrap();
        let read =
            |sql: &str| -> String { connection.query_row(sql, [], |row| row.get(0)).unwrap() };
        assert_eq!(read("SELECT text FROM instance_notes"), "Inclui índice.");
        assert_eq!(read("SELECT audience FROM works"), "Ensino médio");
        let description =
            |entity, id| read_description(&connection, EntityRef { entity, id }).unwrap();
        let work = description(EntityKind::Work, 1);
        assert_eq!(work.classes, ["Text", "Monograph"]);
        assert_eq!(work.statements, [concept("content", "contentTypes", "txt")]);
        assert_eq!(
            description(EntityKind::Instance, 1).statements,
            [concept("issuance", "issuance", "mono")]
        );
    }

    #[test]
    fn migration_18_reads_the_other_titles_from_the_source_record() {
        use elzevir_marc::{Field, Record, Subfield, Tag};
        let field = |tag: &[u8; 3], indicators: &[u8; 2], text: &str| Field::Data {
            tag: Tag::new(*tag),
            indicators: *indicators,
            subfields: vec![Subfield {
                code: b'a',
                value: text.as_bytes().to_vec(),
            }],
        };
        let mut source = Vec::new();
        let record = Record {
            leader: *b"00000nam a2200000   4500",
            fields: vec![
                field(b"245", b"10", "Dom Casmurro"),
                field(b"246", b"3 ", "Capitu"),
                field(b"246", b"18", "D. Casmurro"),
            ],
        };
        record.write_iso2709(&mut source).unwrap();
        let (_dir, mut connection) = catalog();
        connection
            .execute(
                "UPDATE source_records SET record = ?1 WHERE instance_id = 1",
                params![source],
            )
            .unwrap();
        rewind(&connection, TITLES_VERSION - 1);
        migrate(&mut connection).unwrap();
        let titles = |entity| {
            read_description(&connection, EntityRef { entity, id: 1 })
                .unwrap()
                .statements
                .into_iter()
                .filter(elzevir_domain::statement::is_other_title)
                .count()
        };
        assert_eq!(
            (titles(EntityKind::Work), titles(EntityKind::Instance)),
            (1, 1)
        );
        // Titles a record already has are not added again.
        backfill_from_source(&connection, source_titles).unwrap();
        assert_eq!(
            (titles(EntityKind::Work), titles(EntityKind::Instance)),
            (1, 1)
        );
    }

    #[test]
    fn migration_19_reads_the_typed_notes_from_the_source_record() {
        use elzevir_app::description::typed_notes;
        use elzevir_marc::{Field, Record, Subfield, Tag};
        let field = |tag: &[u8; 3], text: &str| Field::Data {
            tag: Tag::new(*tag),
            indicators: *b"  ",
            subfields: vec![Subfield {
                code: b'a',
                value: text.as_bytes().to_vec(),
            }],
        };
        let mut source = Vec::new();
        let record = Record {
            leader: *b"00000nam a2200000   4500",
            fields: vec![
                field(b"245", "Dom Casmurro"),
                field(b"504", "Inclui bibliografia."),
                field(b"546", "Em português."),
                field(b"561", "Doação."),
            ],
        };
        record.write_iso2709(&mut source).unwrap();
        let (_dir, mut connection) = catalog();
        connection
            .execute(
                "UPDATE source_records SET record = ?1 WHERE instance_id = 1",
                params![source],
            )
            .unwrap();
        rewind(&connection, TYPED_NOTES_VERSION - 1);
        migrate(&mut connection).unwrap();
        let description =
            |entity| read_description(&connection, EntityRef { entity, id: 1 }).unwrap();
        let (work, instance) = (
            description(EntityKind::Work),
            description(EntityKind::Instance),
        );
        assert_eq!(typed_notes(&[&work]), ["Em português."]);
        assert_eq!(typed_notes(&[&instance]), ["Inclui bibliografia."]);
        assert!(
            instance
                .statements
                .iter()
                .any(|statement| statement.property == "custodialHistory")
        );
    }

    #[test]
    fn migration_20_keeps_every_place_and_the_manufacture_but_not_over_an_edit() {
        use elzevir_marc::{Field, Record, Subfield, Tag};
        let subfield = |code: u8, value: &str| Subfield {
            code,
            value: value.as_bytes().to_vec(),
        };
        let mut source = Vec::new();
        let record = Record {
            leader: *b"00000nam a2200000   4500",
            fields: vec![
                Field::Data {
                    tag: Tag::new(*b"245"),
                    indicators: *b"10",
                    subfields: vec![subfield(b'a', "Dom Casmurro")],
                },
                Field::Data {
                    tag: Tag::new(*b"260"),
                    indicators: *b"  ",
                    subfields: vec![
                        subfield(b'a', "São Paulo :"),
                        subfield(b'a', "Curitiba :"),
                        subfield(b'b', "EdUSP :"),
                        subfield(b'b', "FAPESP,"),
                        subfield(b'c', "2000"),
                        subfield(b'f', "(Gráfica)"),
                    ],
                },
            ],
        };
        record.write_iso2709(&mut source).unwrap();
        let (_dir, mut connection) = catalog();
        connection
            .execute(
                "UPDATE source_records SET record = ?1 WHERE instance_id = 1",
                params![source],
            )
            .unwrap();
        // As part 5c found it: the first place, and a publisher edited since the import.
        connection
            .execute(
                "UPDATE instances SET place = 'São Paulo', publisher = 'Editada' WHERE id = 1",
                [],
            )
            .unwrap();
        rewind(&connection, PROVISION_VERSION - 1);
        migrate(&mut connection).unwrap();
        let (place, publisher): (String, String) = connection
            .query_row(
                "SELECT place, publisher FROM instances WHERE id = 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(
            (place.as_str(), publisher.as_str()),
            ("São Paulo ; Curitiba", "Editada")
        );
        let instance = read_description(
            &connection,
            EntityRef {
                entity: EntityKind::Instance,
                id: 1,
            },
        )
        .unwrap();
        assert!(
            instance
                .statements
                .iter()
                .any(elzevir_domain::statement::is_other_provision)
        );
    }

    #[test]
    fn migration_21_reads_series_and_related_works_from_the_source_record() {
        use elzevir_marc::{Field, Record, Subfield, Tag};
        let data = |tag: [u8; 3], indicators: [u8; 2], value: &str| Field::Data {
            tag: Tag::new(tag),
            indicators,
            subfields: vec![Subfield {
                code: b'a',
                value: value.as_bytes().to_vec(),
            }],
        };
        let mut source = Vec::new();
        let record = Record {
            leader: *b"00000nam a2200000   4500",
            fields: vec![
                data(*b"245", *b"10", "Dom Casmurro"),
                data(*b"740", *b"02", "O Alienista"),
                data(*b"830", *b" 0", "Grandes vultos"),
            ],
        };
        record.write_iso2709(&mut source).unwrap();
        let (_dir, mut connection) = catalog();
        connection
            .execute(
                "UPDATE source_records SET record = ?1 WHERE instance_id = 1",
                params![source],
            )
            .unwrap();
        rewind(&connection, RELATIONS_VERSION - 1);
        migrate(&mut connection).unwrap();
        let work = read_description(
            &connection,
            EntityRef {
                entity: EntityKind::Work,
                id: 1,
            },
        )
        .unwrap();
        let relations = work
            .statements
            .iter()
            .filter(|statement| statement.property == "relation")
            .count();
        assert_eq!(relations, 2);
    }

    #[test]
    fn migration_22_reads_coverage_parts_and_locations_from_the_source_record() {
        use elzevir_marc::{Field, Record, Subfield, Tag};
        let data = |tag: [u8; 3], indicators: [u8; 2], subfields: &[(u8, &str)]| Field::Data {
            tag: Tag::new(tag),
            indicators,
            subfields: subfields
                .iter()
                .map(|(code, value)| Subfield {
                    code: *code,
                    value: value.as_bytes().to_vec(),
                })
                .collect(),
        };
        let mut source = Vec::new();
        let record = Record {
            leader: *b"00000nam a2200000   4500",
            fields: vec![
                data(*b"041", *b"1 ", &[(b'a', "por"), (b'h', "fre")]),
                data(*b"043", *b"  ", &[(b'a', "s-bl---")]),
                data(*b"245", *b"10", &[(b'a', "Dom Casmurro")]),
                data(*b"856", *b"4 ", &[(b'u', "https://x.org/capa.png")]),
            ],
        };
        record.write_iso2709(&mut source).unwrap();
        let (_dir, mut connection) = catalog();
        connection
            .execute(
                "UPDATE source_records SET record = ?1 WHERE instance_id = 1",
                params![source],
            )
            .unwrap();
        rewind(&connection, CONTENT_VERSION - 1);
        migrate(&mut connection).unwrap();
        let properties = |entity| {
            read_description(&connection, EntityRef { entity, id: 1 })
                .unwrap()
                .statements
                .into_iter()
                .map(|statement| statement.property)
                .collect::<Vec<_>>()
        };
        assert_eq!(
            properties(EntityKind::Work),
            ["geographicCoverage", "language"]
        );
        assert_eq!(properties(EntityKind::Instance), ["electronicLocator"]);
    }

    #[test]
    fn migration_23_splits_the_extent_but_not_an_edited_one() {
        use elzevir_marc::{Field, Record, Subfield, Tag};
        let source_of = |extent: &str| {
            let mut source = Vec::new();
            Record {
                leader: *b"00000nam a2200000   4500",
                fields: vec![
                    Field::Data {
                        tag: Tag::new(*b"245"),
                        indicators: *b"10",
                        subfields: vec![Subfield {
                            code: b'a',
                            value: b"Dom Casmurro".to_vec(),
                        }],
                    },
                    Field::Data {
                        tag: Tag::new(*b"300"),
                        indicators: *b"  ",
                        subfields: [(b'a', extent), (b'b', "il. ;"), (b'c', "19 cm.")]
                            .iter()
                            .map(|(code, value)| Subfield {
                                code: *code,
                                value: value.as_bytes().to_vec(),
                            })
                            .collect(),
                    },
                ],
            }
            .write_iso2709(&mut source)
            .unwrap();
            source
        };
        let (_dir, mut connection) = catalog();
        connection
            .execute_batch(
                "INSERT INTO instances (id, work_id, title) VALUES (2, 1, 'Dom Casmurro');
                 INSERT INTO source_records (instance_id, format, record) VALUES (2, 1, x'00');
                 UPDATE instances SET extent = '195 p. : il. ; 19 cm.' WHERE id = 1;
                 UPDATE instances SET extent = 'Editada' WHERE id = 2;",
            )
            .unwrap();
        for id in [1, 2] {
            connection
                .execute(
                    "UPDATE source_records SET record = ?2 WHERE instance_id = ?1",
                    params![id, source_of("195 p. :")],
                )
                .unwrap();
        }
        rewind(&connection, PHYSICAL_VERSION - 1);
        migrate(&mut connection).unwrap();
        let read = |id: i64| {
            let extent: Option<String> = connection
                .query_row("SELECT extent FROM instances WHERE id = ?1", [id], |row| {
                    row.get(0)
                })
                .unwrap();
            let statements = read_description(
                &connection,
                EntityRef {
                    entity: EntityKind::Instance,
                    id,
                },
            )
            .unwrap()
            .statements
            .len();
            (extent, statements)
        };
        assert_eq!(read(1), (Some("195 p.".to_owned()), 2));
        assert_eq!(read(2), (Some("Editada".to_owned()), 0));
    }

    #[test]
    fn migration_24_reads_the_admin_metadata_from_the_source_record() {
        use elzevir_marc::{Field, Record, Subfield, Tag};
        let mut source = Vec::new();
        Record {
            leader: *b"00000nam a2200000   4500",
            fields: vec![
                Field::Control {
                    tag: Tag::new(*b"001"),
                    value: b"42".to_vec(),
                },
                Field::Data {
                    tag: Tag::new(*b"245"),
                    indicators: *b"10",
                    subfields: vec![Subfield {
                        code: b'a',
                        value: b"Dom Casmurro".to_vec(),
                    }],
                },
            ],
        }
        .write_iso2709(&mut source)
        .unwrap();
        let (_dir, mut connection) = catalog();
        connection
            .execute(
                "UPDATE source_records SET record = ?1 WHERE instance_id = 1",
                params![source],
            )
            .unwrap();
        rewind(&connection, ADMIN_VERSION - 1);
        migrate(&mut connection).unwrap();
        let instance = read_description(
            &connection,
            EntityRef {
                entity: EntityKind::Instance,
                id: 1,
            },
        )
        .unwrap();
        assert!(
            instance
                .statements
                .iter()
                .any(|statement| statement.property == "adminMetadata")
        );
    }

    #[test]
    fn migration_31_adds_the_conventions_of_the_leader_to_the_admin_metadata() {
        use elzevir_marc::{Field, Record};
        let source = |leader: &[u8; 24]| {
            let mut bytes = Vec::new();
            Record {
                leader: *leader,
                fields: vec![Field::Control {
                    tag: elzevir_marc::Tag::new(*b"001"),
                    value: b"42".to_vec(),
                }],
            }
            .write_iso2709(&mut bytes)
            .unwrap();
            bytes
        };
        let (_dir, mut connection) = catalog();
        let instance = EntityRef {
            entity: EntityKind::Instance,
            id: 1,
        };
        // The administrative metadata as stored before: Leader/18 was not read.
        let (_, before) = source_admin(&source(b"00000nam a2200000   4500")).unwrap();
        store_description(
            &connection,
            instance,
            &Description {
                classes: Vec::new(),
                statements: before,
            },
        )
        .unwrap();
        connection
            .execute(
                "UPDATE source_records SET record = ?1 WHERE instance_id = 1",
                params![source(b"00000nam a2200000 a 4500")],
            )
            .unwrap();
        rewind(&connection, LEADER_CONVENTIONS_VERSION - 1);
        migrate(&mut connection).unwrap();
        let (_, wanted) = source_admin(&source(b"00000nam a2200000 a 4500")).unwrap();
        let admin: Vec<Statement> = read_description(&connection, instance)
            .unwrap()
            .statements
            .into_iter()
            .filter(|s| s.property == "adminMetadata")
            .collect();
        assert_eq!(admin, wanted);
    }

    #[test]
    fn migration_35_reads_the_forms_of_the_work_from_the_source_record() {
        use elzevir_marc::{Field, Record, Subfield, Tag};
        let mut source = Vec::new();
        Record {
            leader: *b"00000nam a2200000   4500",
            fields: vec![Field::Data {
                tag: Tag::new(*b"380"),
                indicators: *b"  ",
                subfields: vec![Subfield {
                    code: b'a',
                    value: "Ficção".as_bytes().to_vec(),
                }],
            }],
        }
        .write_iso2709(&mut source)
        .unwrap();
        let (_dir, mut connection) = catalog();
        connection
            .execute(
                "UPDATE source_records SET record = ?1 WHERE instance_id = 1",
                params![source],
            )
            .unwrap();
        rewind(&connection, FORMS_VERSION - 1);
        migrate(&mut connection).unwrap();
        let work = read_description(
            &connection,
            EntityRef {
                entity: EntityKind::Work,
                id: 1,
            },
        )
        .unwrap();
        let (wanted, _) = source_forms(&source).unwrap();
        assert_eq!(wanted.len(), 1);
        assert!(wanted.iter().all(|s| work.statements.contains(s)));
    }

    #[test]
    fn migration_34_reads_the_time_periods_and_national_numbers_from_the_source_record() {
        use elzevir_marc::{Field, Record, Subfield, Tag};
        let subfield = |code: u8, value: &str| Subfield {
            code,
            value: value.as_bytes().to_vec(),
        };
        let mut source = Vec::new();
        Record {
            leader: *b"00000nam a2200000   4500",
            fields: vec![
                Field::Data {
                    tag: Tag::new(*b"016"),
                    indicators: *b"7 ",
                    subfields: vec![subfield(b'a', "0000391964"), subfield(b'2', "BR-RjBN")],
                },
                Field::Data {
                    tag: Tag::new(*b"045"),
                    indicators: *b"  ",
                    subfields: vec![subfield(b'a', "x9x9")],
                },
            ],
        }
        .write_iso2709(&mut source)
        .unwrap();
        let (_dir, mut connection) = catalog();
        connection
            .execute(
                "UPDATE source_records SET record = ?1 WHERE instance_id = 1",
                params![source],
            )
            .unwrap();
        rewind(&connection, PERIODS_VERSION - 1);
        migrate(&mut connection).unwrap();
        let read = |entity| {
            read_description(&connection, EntityRef { entity, id: 1 })
                .unwrap()
                .statements
        };
        let (work, instance) = source_periods_and_numbers(&source).unwrap();
        assert_eq!((work.len(), instance.len()), (1, 1));
        let (stored_work, stored_instance) = (read(EntityKind::Work), read(EntityKind::Instance));
        assert!(work.iter().all(|s| stored_work.contains(s)));
        assert!(instance.iter().all(|s| stored_instance.contains(s)));
    }

    #[test]
    fn migration_33_reads_the_positions_of_a_book_from_the_source_record() {
        use elzevir_marc::{Field, Record, Tag};
        let mut source = Vec::new();
        Record {
            leader: *b"00000nam a2200000   4500",
            fields: vec![Field::Control {
                tag: Tag::new(*b"008"),
                value: b"250321s1995    bl ac  j bd   111 1bpor d".to_vec(),
            }],
        }
        .write_iso2709(&mut source)
        .unwrap();
        let (_dir, mut connection) = catalog();
        connection
            .execute(
                "UPDATE source_records SET record = ?1 WHERE instance_id = 1",
                params![source],
            )
            .unwrap();
        rewind(&connection, BOOKS_VERSION - 1);
        migrate(&mut connection).unwrap();
        let work = read_description(
            &connection,
            EntityRef {
                entity: EntityKind::Work,
                id: 1,
            },
        )
        .unwrap();
        let (wanted, _) = source_books(&source).unwrap();
        assert_eq!(wanted.len(), 10);
        assert!(
            wanted
                .iter()
                .all(|statement| work.statements.contains(statement))
        );
    }

    #[test]
    fn migration_32_reads_the_classifications_from_the_source_record() {
        use elzevir_marc::{Field, Record, Subfield, Tag};
        let subfield = |code: u8, value: &str| Subfield {
            code,
            value: value.as_bytes().to_vec(),
        };
        let mut source = Vec::new();
        Record {
            leader: *b"00000nam a2200000   4500",
            fields: vec![
                Field::Data {
                    tag: Tag::new(*b"080"),
                    indicators: *b"  ",
                    subfields: vec![subfield(b'a', "316.7")],
                },
                Field::Data {
                    tag: Tag::new(*b"082"),
                    indicators: *b"04",
                    subfields: vec![subfield(b'a', "320.5"), subfield(b'2', "23")],
                },
            ],
        }
        .write_iso2709(&mut source)
        .unwrap();
        let (_dir, mut connection) = catalog();
        connection
            .execute(
                "UPDATE source_records SET record = ?1 WHERE instance_id = 1",
                params![source],
            )
            .unwrap();
        rewind(&connection, CLASSIFICATIONS_VERSION - 1);
        migrate(&mut connection).unwrap();
        let work = read_description(
            &connection,
            EntityRef {
                entity: EntityKind::Work,
                id: 1,
            },
        )
        .unwrap();
        // The UDC whole; of the DDC, the rest of the field (the number is the Work's own).
        let (wanted, _) = source_classification(&source).unwrap();
        let classifications: Vec<Statement> = work
            .statements
            .into_iter()
            .filter(|s| s.property == "classification")
            .collect();
        assert_eq!(classifications, wanted);
        assert_eq!(classifications.len(), 2);
    }

    #[test]
    fn migration_25_reads_the_coded_publication_from_the_source_record() {
        use elzevir_marc::{Field, Record, Subfield, Tag};
        let mut source = Vec::new();
        Record {
            leader: *b"00000nam a2200000   4500",
            fields: vec![
                Field::Control {
                    tag: Tag::new(*b"008"),
                    value: b"870910r19849999bl            000 0 por d".to_vec(),
                },
                Field::Data {
                    tag: Tag::new(*b"245"),
                    indicators: *b"10",
                    subfields: vec![Subfield {
                        code: b'a',
                        value: b"Dom Casmurro".to_vec(),
                    }],
                },
            ],
        }
        .write_iso2709(&mut source)
        .unwrap();
        let (_dir, mut connection) = catalog();
        connection
            .execute(
                "UPDATE source_records SET record = ?1 WHERE instance_id = 1",
                params![source],
            )
            .unwrap();
        rewind(&connection, FIXED_VERSION - 1);
        migrate(&mut connection).unwrap();
        let properties = |entity| {
            read_description(&connection, EntityRef { entity, id: 1 })
                .unwrap()
                .statements
                .into_iter()
                .map(|statement| statement.property)
                .collect::<Vec<_>>()
        };
        assert_eq!(properties(EntityKind::Work), ["originDate"]);
        assert_eq!(properties(EntityKind::Instance), ["provisionActivity"]);
    }

    #[test]
    fn migration_26_reads_the_statement_of_responsibility_from_the_source_record() {
        use elzevir_marc::{Field, Record, Subfield, Tag};
        let mut source = Vec::new();
        Record {
            leader: *b"00000nam a2200000   4500",
            fields: vec![Field::Data {
                tag: Tag::new(*b"245"),
                indicators: *b"10",
                subfields: vec![
                    Subfield {
                        code: b'a',
                        value: b"Dom Casmurro /".to_vec(),
                    },
                    Subfield {
                        code: b'c',
                        value: b"Machado de Assis.".to_vec(),
                    },
                ],
            }],
        }
        .write_iso2709(&mut source)
        .unwrap();
        let (_dir, mut connection) = catalog();
        connection
            .execute(
                "UPDATE source_records SET record = ?1 WHERE instance_id = 1",
                params![source],
            )
            .unwrap();
        rewind(&connection, RESPONSIBILITY_VERSION - 1);
        migrate(&mut connection).unwrap();
        let instance = read_description(
            &connection,
            EntityRef {
                entity: EntityKind::Instance,
                id: 1,
            },
        )
        .unwrap();
        assert_eq!(
            instance.statements,
            [Statement {
                property: "responsibilityStatement".to_owned(),
                value: Value::Literal {
                    text: "Machado de Assis".to_owned(),
                    language: None,
                },
            }]
        );
    }

    #[test]
    fn migration_27_reads_what_the_subdivisions_name_from_the_source_record() {
        use elzevir_marc::{Field, Record, Subfield, Tag};
        let subfield = |code: u8, value: &str| Subfield {
            code,
            value: value.as_bytes().to_vec(),
        };
        let mut source = Vec::new();
        Record {
            leader: *b"00000nam a2200000   4500",
            fields: vec![Field::Data {
                tag: Tag::new(*b"650"),
                indicators: *b" 4",
                subfields: vec![subfield(b'a', "Racismo"), subfield(b'z', "Estados Unidos.")],
            }],
        }
        .write_iso2709(&mut source)
        .unwrap();
        let (_dir, mut connection) = catalog();
        connection
            .execute_batch(
                "INSERT INTO subjects (id, kind, heading) VALUES (1, 1, 'Racismo -- Estados Unidos');
                 INSERT INTO work_subjects (work_id, position, subject_id) VALUES (1, 0, 1);",
            )
            .unwrap();
        connection
            .execute(
                "UPDATE source_records SET record = ?1 WHERE instance_id = 1",
                params![source],
            )
            .unwrap();
        rewind(&connection, SUBDIVISIONS_VERSION - 1);
        migrate(&mut connection).unwrap();
        let subdivisions: String = connection
            .query_row(
                "SELECT s.subdivisions FROM work_subjects ws JOIN subjects s ON s.id = ws.subject_id
                 WHERE ws.work_id = 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(subdivisions, SubjectKind::Place.code().to_string());
    }

    #[test]
    fn migration_28_reads_the_name_parts_from_the_source_record() {
        use elzevir_marc::{Field, Record, Subfield, Tag};
        let subfield = |code: u8, value: &str| Subfield {
            code,
            value: value.as_bytes().to_vec(),
        };
        let mut source = Vec::new();
        Record {
            leader: *b"00000nam a2200000   4500",
            fields: vec![Field::Data {
                tag: Tag::new(*b"100"),
                indicators: *b"1 ",
                subfields: vec![
                    subfield(b'a', "Assis, Machado de,"),
                    subfield(b'd', "1839-1908."),
                ],
            }],
        }
        .write_iso2709(&mut source)
        .unwrap();
        let (_dir, mut connection) = catalog();
        connection
            .execute_batch(
                "INSERT INTO agents (id, kind, heading) VALUES (1, 1, 'Assis, Machado de, 1839-1908');
                 INSERT INTO contributions (work_id, position, agent_id, is_primary)
                     VALUES (1, 0, 1, 1);",
            )
            .unwrap();
        connection
            .execute(
                "UPDATE source_records SET record = ?1 WHERE instance_id = 1",
                params![source],
            )
            .unwrap();
        rewind(&connection, NAME_PARTS_VERSION - 1);
        migrate(&mut connection).unwrap();
        let parts: String = connection
            .query_row("SELECT name_parts FROM agents WHERE id = 1", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(parts, "3@19");
    }

    #[test]
    fn migration_29_reads_the_authority_links_and_relator_terms_from_the_source_record() {
        use elzevir_marc::{Field, Record, Subfield, Tag};
        let subfield = |code: u8, value: &str| Subfield {
            code,
            value: value.as_bytes().to_vec(),
        };
        let mut source = Vec::new();
        Record {
            leader: *b"00000nam a2200000   4500",
            fields: vec![Field::Data {
                tag: Tag::new(*b"100"),
                indicators: *b"1 ",
                subfields: vec![
                    subfield(b'a', "Assis, Machado de,"),
                    subfield(b'e', "autor"),
                    subfield(b'0', "http://id.loc.gov/authorities/names/n80022867"),
                    subfield(b'1', "http://viaf.org/viaf/49225025"),
                    subfield(b'4', "aut"),
                ],
            }],
        }
        .write_iso2709(&mut source)
        .unwrap();
        let (_dir, mut connection) = catalog();
        connection
            .execute_batch(
                "INSERT INTO agents (id, kind, heading) VALUES (1, 1, 'Assis, Machado de');
                 INSERT INTO contributions (work_id, position, agent_id, is_primary)
                     VALUES (1, 0, 1, 1);",
            )
            .unwrap();
        connection
            .execute(
                "UPDATE source_records SET record = ?1 WHERE instance_id = 1",
                params![source],
            )
            .unwrap();
        rewind(&connection, AGENT_IDENTIFIERS_VERSION - 1);
        migrate(&mut connection).unwrap();
        let term: Option<String> = connection
            .query_row(
                "SELECT role_term FROM contributions WHERE work_id = 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(term.as_deref(), Some("autor"));
        let links: Vec<(u8, String)> = connection
            .prepare(
                "SELECT kind, value FROM contribution_identifiers
                 WHERE work_id = 1 AND contribution = 0 ORDER BY position",
            )
            .unwrap()
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(
            links,
            [
                (
                    1,
                    "http://id.loc.gov/authorities/names/n80022867".to_owned()
                ),
                (2, "http://viaf.org/viaf/49225025".to_owned()),
            ]
        );
    }

    #[test]
    fn migration_30_tells_an_organization_subject_from_a_person_and_reads_its_parts_and_links() {
        use elzevir_marc::{Field, Record, Subfield, Tag};
        let subfield = |code: u8, value: &str| Subfield {
            code,
            value: value.as_bytes().to_vec(),
        };
        let mut source = Vec::new();
        Record {
            leader: *b"00000nam a2200000   4500",
            fields: vec![Field::Data {
                tag: Tag::new(*b"610"),
                indicators: *b"14",
                subfields: vec![
                    subfield(b'a', "Brasil."),
                    subfield(b'b', "Senado Federal"),
                    subfield(b'0', "http://id.loc.gov/authorities/names/n79059917"),
                ],
            }],
        }
        .write_iso2709(&mut source)
        .unwrap();
        let (_dir, mut connection) = catalog();
        // Before migration 30, any name as subject had kind 3.
        connection
            .execute_batch(
                "INSERT INTO subjects (id, kind, heading) VALUES (1, 3, 'Brasil. Senado Federal');
                 INSERT INTO work_subjects (work_id, position, subject_id) VALUES (1, 0, 1);",
            )
            .unwrap();
        connection
            .execute(
                "UPDATE source_records SET record = ?1 WHERE instance_id = 1",
                params![source],
            )
            .unwrap();
        rewind(&connection, SUBJECT_AGENTS_VERSION - 1);
        migrate(&mut connection).unwrap();
        let (kind, parts): (u8, String) = connection
            .query_row(
                "SELECT s.kind, s.name_parts FROM work_subjects ws
                 JOIN subjects s ON s.id = ws.subject_id WHERE ws.work_id = 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(kind, SubjectKind::Organization.code());
        assert_eq!(parts, "5@8");
        let link: (u8, String) = connection
            .query_row(
                "SELECT kind, value FROM subject_identifiers WHERE work_id = 1 AND subject = 0",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(
            link,
            (
                1,
                "http://id.loc.gov/authorities/names/n79059917".to_owned()
            )
        );
    }

    /// Takes a catalog back to schema `version`, undoing the migrations after it that change
    /// tables, so they can run again as on a database stored before them.
    #[test]
    fn migration_36_keeps_a_digital_edition_as_electronic_and_drops_the_category() {
        let (_dir, mut connection) = catalog();
        rewind(&connection, NO_CATEGORY_VERSION - 1);
        connection
            .execute_batch(
                "UPDATE works SET material_type = 1; UPDATE instances SET material_type = 7;",
            )
            .unwrap();
        migrate(&mut connection).unwrap();
        let instance = EntityRef {
            entity: EntityKind::Instance,
            id: 1,
        };
        let classes = read_description(&connection, instance).unwrap().classes;
        assert!(classes.contains(&"Electronic".to_owned()), "{classes:?}");
        for table in ["works", "instances"] {
            let category: i64 = connection
                .query_row(
                    &format!(
                        "SELECT COUNT(*) FROM pragma_table_info('{table}')
                         WHERE name = 'material_type'"
                    ),
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(category, 0, "{table}");
        }
    }

    fn rewind(connection: &Connection, version: i64) {
        connection
            .execute_batch(include_str!("../tests/data/rewind_repository.sql"))
            .unwrap();
        connection
            .execute_batch("DROP TABLE IF EXISTS import_identities")
            .unwrap();
        if version < WORK_LISTS_VERSION {
            connection
                .execute_batch(
                    "DROP TABLE board_notes; DROP TABLE notice_texts; DROP TABLE hold_notice_preferences; DROP TABLE daily_runs; DROP TABLE notice_items; DROP TABLE notices; DROP TABLE overdue_notice_rules; DROP TABLE ended_holds; DROP TABLE holds; DROP TABLE account_offsets; DROP TABLE account_lines; DROP TABLE patron_restrictions; DROP TABLE transfers; DROP TABLE returned_loans; DROP TABLE loans; DROP TABLE calendar_dates; DROP TABLE calendar_annual; DROP TABLE calendar_weekly; DROP TABLE circulation_rules; DROP TABLE patrons; DROP TABLE patron_categories; DROP INDEX audit_imports; DROP TABLE saved_list_families; DROP TABLE saved_lists; DROP INDEX audit_by_actor; DROP INDEX audit_by_kind;
                     DROP INDEX audit_entities_by_seq;",
                )
                .unwrap();
        }
        if version < SUBJECT_VERSION {
            connection
                .execute_batch("ALTER TABLE subjects DROP COLUMN row_version;")
                .unwrap();
        }
        if version < AGENT_VERSION {
            connection
                .execute_batch("ALTER TABLE agents DROP COLUMN row_version;")
                .unwrap();
        }
        if version < OPAC_HIDDEN_VERSION {
            connection
                .execute_batch("ALTER TABLE instances DROP COLUMN opac_hidden;")
                .unwrap();
        }
        if version < HISTORY_VERSION {
            connection
                .execute_batch(
                    "DROP TABLE audit_entities;
                     ALTER TABLE audit_log DROP COLUMN at;",
                )
                .unwrap();
        }
        if version < NO_CATEGORY_VERSION {
            connection
                .execute_batch(
                    "ALTER TABLE works ADD COLUMN material_type INTEGER;
                     ALTER TABLE instances ADD COLUMN material_type INTEGER;",
                )
                .unwrap();
        }
        if version < SUBJECT_AGENTS_VERSION {
            connection
                .execute_batch(
                    "DROP TABLE subject_identifiers;
                     ALTER TABLE subjects DROP COLUMN name_parts;",
                )
                .unwrap();
        }
        if version < AGENT_IDENTIFIERS_VERSION {
            connection
                .execute_batch(
                    "DROP TABLE contribution_identifiers;
                     ALTER TABLE contributions DROP COLUMN role_term;",
                )
                .unwrap();
        }
        if version < NAME_PARTS_VERSION {
            connection
                .execute_batch("ALTER TABLE agents DROP COLUMN name_parts;")
                .unwrap();
        }
        if version < SUBDIVISIONS_VERSION {
            connection
                .execute_batch(
                    "DROP INDEX subjects_identity;
                     ALTER TABLE subjects DROP COLUMN subdivisions;
                     CREATE UNIQUE INDEX subjects_identity
                         ON subjects (kind, heading, coalesce(thesaurus, 0));",
                )
                .unwrap();
        }
        connection
            .pragma_update(None, "user_version", version)
            .unwrap();
    }

    fn catalog() -> (tempfile::TempDir, Connection) {
        let dir = tempfile::tempdir().unwrap();
        let connection = open_writer(&dir.path().join("elzevir.db")).unwrap();
        connection
            .execute_batch(
                "INSERT INTO branches (id, name) VALUES (1, 'Unit');
                 INSERT INTO circulation_policies (id, name) VALUES (1, 'Policy');
                 INSERT INTO works (id, title) VALUES (1, 'Dom Casmurro');
                 INSERT INTO instances (id, work_id, title) VALUES (1, 1, 'Dom Casmurro');
                 INSERT INTO instance_identifiers (instance_id, position, kind, value)
                     VALUES (1, 0, 1, '9788535910667');
                 INSERT INTO source_records (instance_id, format, record) VALUES (1, 1, x'00');",
            )
            .unwrap();
        (dir, connection)
    }

    fn item(
        connection: &Connection,
        instance: i64,
        barcode: Option<&str>,
    ) -> rusqlite::Result<usize> {
        connection.execute(
            "INSERT INTO items (instance_id, barcode, home_branch, holding_branch, policy_id)
             VALUES (?1, ?2, 1, 1, 1)",
            params![instance, barcode],
        )
    }

    #[test]
    fn deleting_what_is_in_use_is_refused() {
        let (_dir, connection) = catalog();
        item(&connection, 1, Some("1511")).unwrap();
        assert!(
            connection
                .execute("DELETE FROM works WHERE id = 1", [])
                .is_err()
        );
        assert!(
            connection
                .execute("DELETE FROM instances WHERE id = 1", [])
                .is_err()
        );
        assert!(
            connection
                .execute("DELETE FROM branches WHERE id = 1", [])
                .is_err()
        );
        assert!(
            connection
                .execute("DELETE FROM circulation_policies WHERE id = 1", [])
                .is_err()
        );
    }

    #[test]
    fn deleting_an_instance_without_items_takes_its_identifiers_and_source() {
        let (_dir, connection) = catalog();
        connection
            .execute("DELETE FROM instances WHERE id = 1", [])
            .unwrap();
        for table in ["instance_identifiers", "source_records"] {
            let count: i64 = connection
                .query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
                    row.get(0)
                })
                .unwrap();
            assert_eq!(count, 0, "{table}");
        }
    }

    #[test]
    fn barcodes_are_unique_but_optional() {
        let (_dir, connection) = catalog();
        item(&connection, 1, Some("1511")).unwrap();
        assert!(item(&connection, 1, Some("1511")).is_err());
        item(&connection, 1, None).unwrap();
        item(&connection, 1, None).unwrap();
    }

    #[test]
    fn identities_of_terms_and_subjects() {
        let (_dir, connection) = catalog();
        // The same code in two vocabularies is two terms.
        connection
            .execute_batch(
                "INSERT INTO terms (vocabulary, code, label) VALUES (3, 'REF', 'Reference');
                 INSERT INTO terms (id, vocabulary, code, label) VALUES (9, 4, 'REF', 'Reference');",
            )
            .unwrap();
        assert!(
            connection
                .execute(
                    "INSERT INTO terms (vocabulary, code, label) VALUES (3, 'REF', 'x')",
                    []
                )
                .is_err()
        );
        let subject = |thesaurus: Option<i64>| {
            connection.execute(
                "INSERT INTO subjects (kind, heading, thesaurus) VALUES (1, 'Literatura', ?1)",
                params![thesaurus],
            )
        };
        subject(None).unwrap();
        subject(Some(9)).unwrap();
        assert!(
            subject(None).is_err(),
            "two subjects without thesaurus are the same"
        );
        assert!(subject(Some(9)).is_err());
    }

    #[test]
    fn work_relations_and_hubs_keep_the_graph_consistent() {
        let (_dir, connection) = catalog();
        connection
            .execute_batch(
                "INSERT INTO hubs (id, title) VALUES (1, 'Dom Casmurro');
                 UPDATE works SET hub_id = 1 WHERE id = 1;
                 INSERT INTO works (id, title, hub_id) VALUES (2, 'Dom Casmurro (inglês)', 1);
                 INSERT INTO work_relations (work_id, position, kind, related_work_id)
                     VALUES (2, 0, 1, 1);",
            )
            .unwrap();
        let relate = |from: i64, to: i64| {
            connection.execute(
                "INSERT INTO work_relations (work_id, position, kind, related_work_id)
                 VALUES (?1, 1, 1, ?2)",
                params![from, to],
            )
        };
        assert!(relate(1, 1).is_err(), "a Work never relates to itself");
        assert!(relate(1, 99).is_err(), "the related Work must exist");
        assert!(
            connection
                .execute("DELETE FROM works WHERE id = 1", [])
                .is_err(),
            "a Work that others relate to cannot be deleted"
        );
        assert!(
            connection
                .execute("DELETE FROM hubs WHERE id = 1", [])
                .is_err(),
            "a Hub with Works cannot be deleted"
        );
        connection
            .execute("DELETE FROM works WHERE id = 2", [])
            .unwrap();
        let relations: i64 = connection
            .query_row("SELECT count(*) FROM work_relations", [], |row| row.get(0))
            .unwrap();
        assert_eq!(relations, 0, "deleting a Work takes its own relations");
    }

    #[test]
    fn inventory_numbers_may_repeat() {
        let (_dir, connection) = catalog();
        for barcode in ["1", "2"] {
            connection
                .execute(
                    "INSERT INTO items (instance_id, barcode, inventory_number, home_branch,
                                        holding_branch, policy_id)
                     VALUES (1, ?1, '2024/0001', 1, 1, 1)",
                    params![barcode],
                )
                .unwrap();
        }
    }

    #[test]
    fn extra_is_dropped_only_when_empty() {
        let (_dir, mut connection) = catalog();
        let columns = |connection: &Connection| -> i64 {
            connection
                .query_row(
                    "SELECT count(*) FROM pragma_table_info('works') WHERE name = 'extra'",
                    [],
                    |row| row.get(0),
                )
                .unwrap()
        };
        assert_eq!(columns(&connection), 0);
        connection
            .execute_batch(
                "DROP TABLE koha_type_statements; ALTER TABLE circulation_policies ADD COLUMN code TEXT; UPDATE circulation_policies SET code = COALESCE((SELECT code FROM koha_codes WHERE kind = 4 AND local_id = circulation_policies.id), 'P' || id); ALTER TABLE circulation_policies RENAME COLUMN name TO description; ALTER TABLE circulation_policies RENAME TO item_types; ALTER TABLE items RENAME COLUMN policy_id TO item_type; DELETE FROM koha_codes WHERE kind = 4; ALTER TABLE branches ADD COLUMN code TEXT; UPDATE branches SET code = COALESCE((SELECT code FROM koha_codes WHERE kind = 1 AND local_id = branches.id), 'B' || id); ALTER TABLE items ADD COLUMN location INTEGER REFERENCES terms (id); ALTER TABLE items ADD COLUMN collection INTEGER REFERENCES terms (id); INSERT OR IGNORE INTO terms (vocabulary, code, label) SELECT 3, k.code, p.name FROM sublocations p JOIN koha_codes k ON k.kind = 2 AND k.local_id = p.id; INSERT OR IGNORE INTO terms (vocabulary, code, label) SELECT 4, k.code, p.name FROM collections p JOIN koha_codes k ON k.kind = 3 AND k.local_id = p.id; UPDATE items SET location = (SELECT t.id FROM terms t JOIN koha_codes k ON k.kind = 2 AND k.code = t.code WHERE t.vocabulary = 3 AND k.local_id = items.sublocation_id), collection = (SELECT t.id FROM terms t JOIN koha_codes k ON k.kind = 3 AND k.code = t.code WHERE t.vocabulary = 4 AND k.local_id = items.collection_id); ALTER TABLE items DROP COLUMN sublocation_id;
                 ALTER TABLE items DROP COLUMN collection_id;
                 DROP TABLE sublocations; DROP TABLE collections; DROP TABLE koha_codes;
                 ALTER TABLE branches DROP COLUMN address; ALTER TABLE branches DROP COLUMN phone;
                 ALTER TABLE branches DROP COLUMN email; ALTER TABLE branches DROP COLUMN url;
                 ALTER TABLE hubs ADD COLUMN extra TEXT NOT NULL DEFAULT '{}';
                 ALTER TABLE works ADD COLUMN extra TEXT NOT NULL DEFAULT '{}';
                 ALTER TABLE instances ADD COLUMN extra TEXT NOT NULL DEFAULT '{}';
                 ALTER TABLE items ADD COLUMN extra TEXT NOT NULL DEFAULT '{}';
                 UPDATE works SET extra = '{\"x\":1}';",
            )
            .unwrap();
        rewind(&connection, 12);
        assert!(migrate(&mut connection).is_err());
        connection
            .execute_batch("UPDATE works SET extra = '{}'")
            .unwrap();
        migrate(&mut connection).unwrap();
        assert_eq!(columns(&connection), 0);
    }
}

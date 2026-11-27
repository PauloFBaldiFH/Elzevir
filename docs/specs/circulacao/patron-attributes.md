# Additional patron fields (v0)

> Status: v0, task 7.1 of [doc 03](../../arquitetura/03-estrategia-reconstrucao.md) (Phase 7, approved by Paulo on 2026-11-27). Base: [patrons](leitores.md), [LGPD](lgpd.md), [patron migration](../migracao/leitores-do-koha.md), gap G1 of the [switch gaps](../migracao/gaps.md). Part a: the fields and their registry. Part b: the values on the patron. Part c: the Koha migration, the check and the SIP2 currency (G9). Criterion: **the fields and the values of the library's Koha arrive in the switch, and the desk uses them as in Koha**.

Koha lets a library define its own patron fields (`borrower_attribute_types`), and each patron holds values for them (`borrower_attributes`). The library's copy has 4 such fields: all choose from a list, 2 are unique, 2 apply to one category only, 3 are shown at checkout and 1 on the OPAC. The Elzevir calls them **additional patron fields**. Like categories, they are a generic registry the library creates; none comes with the Elzevir.

## 1. Behaviour

### The fields (part a)

1. An additional field has:
   - a **name**, 1 to 256 characters, unique among the fields;
   - a **kind**: free text (up to 255 characters, Koha's `varchar(255)`), a date, or a choice from a **list** of options;
   - **repeatable**: whether a patron can hold more than one value (Koha: `repeatable`);
   - **unique**: whether two patrons can share a value (Koha: `unique_id`);
   - **required**: whether the staff form refuses a patron without a value (Koha: `mandatory`);
   - a **category**: the field applies to the patrons of one category, or to all (Koha: `category_code`);
   - **shown on the OPAC**: whether the patron sees it, read only, on their account (Koha: `opac_display`);
   - **searchable**: whether the staff patron search finds patrons by it (Koha: `staff_searchable`).
2. A list field has 1 to 200 options. Each option has a label of 1 to 256 characters, unique within the field, and keeps the order the library gives. A patron's value is the option itself, so renaming an option renames it for every patron who holds it.
3. The registry is at `/equipe/leitores/campos`, with the patrons permission, like the categories (patrons, item 13): it lists each field with its settings and how many patrons hold a value, and creates, saves and deletes. The options are typed one per line. Refusals show on the page itself. The categories page links to it.
4. A change is refused, with the page showing why, when:
   - the name is empty, too long or taken (409 for taken);
   - a list field has no option, more than 200, an empty or long label, or the same label twice;
   - the kind changes while any patron holds a value (Koha has no such guard, and a value of another kind would stop making sense);
   - **repeatable** is turned off while a patron holds two values, or **unique** is turned on while two patrons share one (Koha: `Koha::Patron::Attribute::Type::check_repeatables` and `check_unique_ids`, 409);
   - an option some patron holds is removed (409);
   - the category does not exist.
5. Turning **required** on does not touch patrons who have no value; their next edit asks for one. Limiting a field to a category keeps the values of patrons of other categories until their next edit.
6. Deleting a field is refused while a patron holds a value for it (Koha: `admin/patron-attr-types.pl`, `ERROR_delete_in_use`, 409).
7. Every change goes to the `audit_log` with who made it.

### The values (part b)

8. The patron form shows, after the fixed fields, the additional fields of the patron's category and those of all categories, in name order. A text field is a text box, a date field a date box, a list field a selection. A repeatable field shows its values and one empty box more; at most 20 values per field.
9. The values are saved with the patron, in the same command, transaction and `row_version` as the other fields (patrons, item 7). Texts are trimmed and an empty box is no value. Saving replaces all of the patron's values.
10. The form comes back with what was typed and the reason (422) when a required field has no value, a date is not a date, a text is too long, an option does not belong to the field, or a field that is not repeatable has two values (or any field more than 20); with 409 when a unique value is another patron's (Koha: `Koha::Patron::Attribute::store`, `unique_ok`, `repeatable_ok`, `value_ok`).
11. The values of fields that do not apply to the patron's category are dropped on save, so changing the category drops them without a refusal. The same value twice in one field counts once.
12. The patron record (patrons, item 12) shows the values of every field, a list field by its label. Since the Elzevir's record is also the checkout screen, there is no separate "show at checkout" setting.
13. The patron's OPAC account shows, read only, the fields marked **shown on the OPAC**.
14. The staff patron search (patrons, item 10) also finds a patron by the value of a **searchable** field: the whole value, or with two characters or more, part of it, without telling capitals apart; a list field by its label. The patron search stays in SQLite behind the wall, like the names and the CPF, and does not go into the Tantivy index of the catalog.
15. The LGPD export of a patron (lgpd.md, item 9) includes the values, by field name. Anonymizing (lgpd.md, items 1 to 7) deletes them.

### Koha migration, check and SIP2 currency (part c)

16. `migrate-koha` reads `borrower_attribute_types`, `borrower_attributes` and the `authorised_values` of the categories those types use. Each type becomes a field: `description` as the name (the code, when empty or taken), a list field when it has `authorised_value_category`, a date field when `is_date`, text otherwise; `repeatable`, `unique_id`, `mandatory`, `category_code`, `opac_display` and `staff_searchable` as in item 1. Each authorised value of its category becomes an option, with `lib` as the label (the value itself, when empty). The type's code goes to the edge table `koha_keys`, so a second run adds nothing twice.
17. Each `borrower_attributes` of a migrated patron becomes a value, a list value by its option. A value that breaks a rule of item 10 (an option not in the list, a date that is not one, a second value of a field that is not repeatable, a unique value another patron already holds) is left out and counted by field and reason, without the value.
18. The report counts fields created and options, values migrated and left out, and, for the settings the Elzevir does not keep (`opac_editable`, `opac_mandatory`, `display_checkout`, `searched_by_default`, `class`, `keep_for_pseudonymization` and the branch limits of `borrower_attribute_types_branches`), how many types used each.
19. `verify-koha` compares, field by field, how many patrons hold a value in Koha and in the Elzevir, and the values left out by the migration count as a known difference.
20. The switch reads the active currency (`currency` with `active = 1`) and writes it to `[sip2] currency`, which the SIP2 server sends in `BH` (gap G9). Without an active currency, the setting is left as it is and the report says so.

## 2. Edge cases

1. A list field limited to a category, with a unique option: two patrons of that category cannot both hold it; a patron of another category cannot hold it at all.
2. Two staff save the same patron with different values: the second is refused as stale (patrons, item 7).
3. A field turned unique while two patrons share a value is refused, and the page counts the values shared.
4. A Koha type whose `authorised_value_category` has no values becomes a list field with no option, which item 2 forbids: it becomes a text field instead, and the report counts it.
5. A Koha value of a type limited to another category than the patron's is migrated anyway (Koha keeps it), and the patron's next edit drops it (item 11).

## 3. Origin in Koha

- `installer/data/mysql/kohastructure.sql`: `borrower_attribute_types`, `borrower_attribute_types_branches`, `borrower_attributes`.
- `Koha::Patron::Attribute` (`store`, `repeatable_ok`, `unique_ok`, `value_ok`), `Koha::Patron::Attribute::Type` (`check_repeatables`, `check_unique_ids`), `Koha::Patron::extended_attributes` (the required fields per category), `admin/patron-attr-types.pl` (deleting a field in use).
- `Koha::Acquisition::Currencies->get_active` for the SIP2 currency (`C4::SIP::ILS::Patron`).

## 4. Parity

`free` for the screens. The migration is checked on the oracle's copy with `verify-koha` (item 19). The rules of items 4 and 10 have unit tests, refusal tests and a test of concurrent edits; the search has a store test with fictitious patrons.

## 5. Deviations

- Koha lets the kind of a field change while patrons hold values; the Elzevir refuses (item 4).
- Koha's branch limits, `class`, OPAC editing and `keep_for_pseudonymization` are not kept. OPAC editing is part of A14 (Phase 8); the others are counted by the migration (item 18).

## 6. Out of this version

- The patron editing their own fields on the OPAC (`opac_editable`, `opac_mandatory`): Phase 8, with self-registration.
- Additional fields in the patron spreadsheet import (A12, Phase 8).

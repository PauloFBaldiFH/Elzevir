# Gaps that would stop the switch (v0)

> Status: v0, task 6.6. Base: [doc 03](../../arquitetura/03-estrategia-reconstrucao.md), Phase 6; the parity inventory approved by Paulo on 2026-11-27 (Blocks A, B and C); the runs of tasks 6.1 to 6.5 on the oracle ([switch](troca-do-koha.md), [check](verificacao-do-koha.md), [parity](koha-parity.md), [desk day](desk-day.md), [performance](performance.md)). Task criterion: **the order of Phases 7 to 12 is revised before Phase 7**.

This page lists what the Phase 6 rehearsal showed is still missing for the Biblioteca Pública Municipal Castro Alves to leave Koha, and crosses it with Block A of the inventory. It ends with the proposed order of Phases 7 to 12, which Paulo approves in doc 03.

## 1. Where the evidence comes from

1. **The runs of 6.1 to 6.5 on the oracle's copy** (18,919 records, 30,034 Items). Every difference these runs found was either fixed in its own PR or recorded as an approved deviation. This page lists only what is still open.
2. **What the library uses in Koha**, counted on the copy with one read-only query per table. Only counts and preference values were read, never a row, and nothing was changed.
3. **The limit of the copy.** The copy is sanitized: it has one patron, and its loans, holds, accounts and messages were emptied. So the use of circulation, patron fields and staff accounts cannot be counted on it. Those counts wait for Paulo's machine, where the production dump lives, and come back as aggregates only.

| Counted on the copy | Value |
|---|---|
| Branches | 1 |
| Items withdrawn / lost / damaged / held at another branch | 1,015 / 0 / 0 / 0 |
| Patron attribute types (`borrower_attribute_types`) | 4 |
| News and pages (`additional_contents`) | 16 |
| OPAC theme (`OPACUserCSS`) | in use, with a carousel of new titles |
| Covers | `OpenLibraryCovers` on; 2 local covers |
| Records with `773` / `690` / `653` | 3 / 115 / 21 |
| Notice templates (`letter`) | 112 |
| Subscriptions, serials, suggestions, vendors, orders, budgets, lists | 0 each |
| Course reserves, ILL, on-site checkouts, self-registration | off |
| `EnhancedMessagingPreferences` | on |
| `AutomaticItemReturn` | on, with a single branch |

## 2. The gaps

### Stop the switch: data is lost or the desk can't work

These are gaps where moving to the Elzevir as it is today would lose something the library has, or the desk would have to go back to Koha for something it does every day.

| # | Gap | Evidence | Block A |
|---|---|---|---|
| G1 | **Patron attributes are not migrated.** The 4 attribute types, and the values patrons hold, stay behind in the switch. | 4 types on the copy; `migrate-koha` reads no `borrower_attributes` | A11; closed by task 7.1 ([patron-attributes.md](../circulacao/patron-attributes.md), items 16 to 19) |
| G2 | **Lost and damaged at the desk.** In Koha, marking an Item lost by hand charges the replacement, and finding it again refunds the charge (`lostreturn`). The Elzevir charges only when a loan is long overdue, and has no refund. | The desk day left this out (desk-day.md, §5). The copy has 0 lost Items because it is sanitized, so the use must be counted in production. | A3 |
| G3 | **The list of holds to pull from the shelf.** Without it, a hold placed on an Item on the shelf waits until someone happens to find the Item. | Daily use in Koha (`circ/pendingreserves.pl`); not in the Elzevir | A2 |
| G4 | **Notice before the due date.** With `EnhancedMessagingPreferences` on, patrons may be getting this notice today; after the switch it would stop. | Preference on; the patron preferences are in production | A6, A15 |

### Do not stop the switch, but the public sees them

| # | Gap | Evidence | Block A |
|---|---|---|---|
| G6 | **OPAC news and appearance.** The 16 news items and pages stay behind. The OPAC also loses the theme, the logo and the carousel of new titles. The Elzevir has its own notice board (mural.md), but the switch does not bring `additional_contents` into it. | 16 rows; `OPACUserCSS` in use | A25 |
| G7 | **Covers.** Koha shows covers from the Open Library; the Elzevir shows none. | `OpenLibraryCovers` on | A17 |
| G8 | **Local subjects (`690`) and index terms (`653`)** do not become subjects of the Work. A subject search misses those records. | 115 and 21 records; the search run (desk-day.md, part b) | new; Phase 9 |

### Neither stops the switch nor shows to the public

| # | Gap | Evidence | Block A |
|---|---|---|---|
| G5 | **Staff profiles.** The Elzevir gives a fixed set of permissions (administration, cataloguing, circulation, patrons) account by account. Koha has about 20 groups with sub-permissions. A desk team can start with what exists, but it has no editable profile. | `flags` and `user_permissions` in production | A9 |
| G9 | **The switch does not bring the active currency** into the SIP2 `BH`. The 6.4b run set it by hand. | `REAL` on the copy; desk-day.md, SIP2 run | new; small, goes with G1; closed by task 7.1 (patron-attributes.md, item 20) |
| G10 | **MARC import below the goal:** 960 records/s, against 2,000 records/s with the index. | performance.md, §3 | new; Phase 9 |
| G11 | **The desk was not measured at real volume** (checkout and checkin p95 < 25 ms), because the copy has one patron. | performance.md, §5 | none; a run on Paulo's machine |

### Block A items the copy shows are not needed for this switch

- **A1, transfers between branches.** The library has one branch, and no Item is held away from its home. Transfers matter for networks and for a library that opens a second branch. They stay in the roadmap, at the end of Phase 7.
- **A21 to A24, serials and acquisitions.** The copy has no subscriptions, orders, vendors, budgets or suggestions. `RoutingSerials` and `suggestion` are on, but nothing is in use. Phases 10 and 11 can wait.
- **A5, on-site checkout and local use.** Both are off on the copy.
- **A16, lists.** There are none on the copy.

## 3. Proposed order of Phases 7 to 12

The principle: first, what stops the switch (G1 to G4); then what the public sees (G6 to G8); then the rest of Block A in the approved order. Each item is still a vertical slice, one at a time.

| Phase | Items, in order | Change from the approved roadmap |
|---|---|---|
| 7, circulation of the day | **G1** (A11, patron attributes, with their migration) and G9; **A3** lost and damaged; **A2** holds to pull; **A6** notice before the due date; A4 automatic renewal; A8 void and receipt; A7 cancellation with a reason (suspension is done, #287 and #288); A5; A1 transfers | A11 moves forward from Phase 8; A1 moves to the end of the phase |
| 8, patrons and staff | **A9** profiles and permissions; A15 message preferences; A10 guarantor; A12 import by spreadsheet; A13 self-registration; A14 renew, card and merge | A11 left for Phase 7; A9 first |
| 9, catalog and OPAC | **G6** (A25 news and appearance, with the migration of `additional_contents`); **G7** (A17 covers); **G8** (`690` and `653`); A18 authorities; A19 parts; A20 batch; A27 ABNT card; A16 lists; **G10** import speed, with a bench first | A25 comes forward from Phase 12, because the public sees it on the first day |
| 10, serials | A21, A22 | unchanged |
| 11, acquisitions | A23, A24 | unchanged |
| 12, reports | A26 | A25 left for Phase 9 |

**The switch itself** can happen after Phase 7, once G11 is measured on Paulo's machine. Phases 8 and 9 can then follow with the library already on the Elzevir, as long as the desk agrees to:
- work with the fixed permissions until A9;
- go without covers and the OPAC theme until Phase 9.

## 4. Out of this version

- Counting circulation, patron attributes, notices and staff accounts in production. This is a run on Paulo's machine, with a report of counts only.
- The detailed plan of each phase, which is written in doc 03 before the phase starts, as the method requires.

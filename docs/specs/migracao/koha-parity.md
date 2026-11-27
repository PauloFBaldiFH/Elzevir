# Parity on the switched copy (v0)

> Status: v0, task 6.3c of [doc 03](../../arquitetura/03-estrategia-reconstrucao.md) (Phase 6, approved by Paulo on 2026-11-27). Part 1 (this PR) compares due dates and fines. Part 2 adds renewals and which hold an Item returned fills. Based on: [the switch](troca-do-koha.md), [the verifier](verificacao-do-koha.md), [due dates](../circulacao/vencimento.md), [fines](../circulacao/multa.md). Goal: **every divergence is a bug that gets fixed or an approved deviation, and the report holds no personal data**.

The verifier (6.2) shows that the data arrived. This step shows that the Elzevir **decides** the same as Koha on that data. Koha runs its own functions over a copy of the library, the Elzevir runs its own over the same copy after the switch, and the answers are compared by Koha's numbers.

## 1. Behaviour

### The recorder, on the Koha side

1. `tools/koha-parity/record.pl` runs inside the Koha of the copy (`koha-shell <instance> -c 'perl record.pl ...'`), on Paulo's machine or on the oracle VM, never in the cloud. It **only reads**: it calls Koha's functions and writes nothing to the database.
2. `--now YYYY-MM-DDTHH:MM` fixes the moment of the answers, in the library's time; a space may stand for the `T`, and anything else is refused, so a date alone never passes for midnight. The default is now, cut to the minute. `--every N` keeps one loan in N, in `issue_id` order. The default, 1, keeps them all.
3. For each current loan (`issues`), it computes the due date the loan would get if it were made again at its `issuedate`. The call is `C4::Circulation::CalcDateDue(issuedate, effective item type, branch, patron)`, with the branch given by `CircControl` and `HomeOrHoldingBranch` (`C4::Circulation::_GetCircControlBranch`).
4. For each overdue loan from `C4::Overdues::Getoverdues`, at `--now`, it computes the fine with `C4::Overdues::CalcFine`, with the branch chosen as `misc/cronjobs/fines.pl` chooses it. Lost Items are skipped, as `fines.pl` skips them. This is the amount before `UpdateFine` limits it to `MaxFine`.
5. **Output:** JSON on standard output. It has the Koha version, `now`, and two lists:
   - `due_dates`: `issue_id`, `itemnumber` and the due date as `YYYY-MM-DD HH:MM`;
   - `fines`: `issue_id`, `itemnumber` and the amount in cents.

   No name, card number, barcode or other personal value is written: only Koha's numbers, dates and amounts.

### The comparison, on the Elzevir side

6. `elzevir parity-koha --data-dir DIR ANSWERS.json` reads the recorder's file and the database of the folder after `switch-from-koha`. The folder is locked during the read, as with every command that opens it. It writes nothing.
7. Each answer is found by its `itemnumber`, through `item_origins`, and the loan of that Item in the Elzevir. Using the Item's own loan, the Elzevir computes:
   - **Due date:** its due date from the same start (`loans.issued_at`), by the rules of the holding branch, the patron's category and the Item's policy, and by the calendar of that branch. This is the computation of `circ.checkout`.
   - **Fine:** at `now`, from the migrated due date, as the daily run computes it (`calc_fine`, without `MaxFine`).
8. **Report:** for each question, how many answers came, how many are the same, and how many could not be compared, with the reason. The reasons are: the Item did not come, the Item is not on loan in the Elzevir, or the Elzevir has no due date for it. Then comes each divergence, by `issue_id`, with both values.

   The values are dates and amounts, never personal data. The command ends with an error when there is any divergence or any answer that could not be compared, so that a script can stop on it.

## 2. Koha origin

- `C4::Circulation::CalcDateDue`, `C4::Circulation::_GetCircControlBranch`
- `C4::Overdues::Getoverdues`, `C4::Overdues::CalcFine`, `misc/cronjobs/fines.pl` (the branch of the fine)

## 3. Parity

`required` (due dates and fines). Proof:
- unit tests of the comparison in `elzevir-app::koha_parity`: same answer, different due date, different fine, Item that did not come, Item not on loan;
- a test of the binary (`crates/elzevir/tests/cli.rs`): after the switch of the synthetic dump, a recorder file built by hand from Koha's rules for that dump matches, and a changed date or amount is reported as a divergence;
- the recorder run on the oracle (its own data) and on the copy of the Castro Alves library, whose report (without personal data) Paulo brings to the project.

### First run on the oracle (2026-11-27)

On the oracle's own copy (18,919 records, 30,034 Items, 42 rules, 30 calendar entries, `daysmode` `Datedue` in the rules), 12 loans were made for the test, backdated from 2026-08-28 to 2026-11-27, and the database was restored afterwards. After `switch-from-koha`:
- `verify-koha` found no difference without a cause.
- `parity-koha` found 12 of 12 due dates and 8 of 8 fines the same as Koha's, including the 40.00 cap.

The run found two problems, fixed in the same PR:
- The MARCXML reader refused a subfield coded with a space. Ten records of that catalog have one, and Koha keeps them. Because of that, the whole switch stopped.
- `--now` with a date alone was read as midnight.

## 4. Edge cases

1. A loan migrated after the dump was made, or returned in the Elzevir after the switch, is not on loan there and cannot be compared. Run the comparison right after the switch.
2. `--now` must be the same moment on both sides. The comparison uses the recorder's `now`, never its own clock.
3. A loan whose due date Koha computes differently today from what it stored (because the rules changed after the checkout) is still compared by today's computation. The stored date is the verifier's business (item 13 of [the verifier](verificacao-do-koha.md)).

## 5. Out of scope (part 2)

- Renewals: `C4::Circulation::CanBookBeRenewed`, with its reason.
- Which hold an Item returned fills: `C4::Reserves::CheckReserves`.

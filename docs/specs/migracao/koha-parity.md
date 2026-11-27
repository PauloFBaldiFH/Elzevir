# Parity on the switched copy (v0)

> Status: v0, task 6.3c of [doc 03](../../arquitetura/03-estrategia-reconstrucao.md) (Phase 6, approved by Paulo on 2026-11-27). Part 1 compares due dates and fines. Part 2 adds renewals and which hold an Item returned fills. Based on: [the switch](troca-do-koha.md), [the verifier](verificacao-do-koha.md), [due dates](../circulacao/vencimento.md), [fines](../circulacao/multa.md). Goal: **every divergence is a bug that gets fixed or an approved deviation, and the report holds no personal data**.

The verifier (6.2) shows that the data arrived. This step shows that the Elzevir **decides** the same as Koha on that data. Koha runs its own functions over a copy of the library, the Elzevir runs its own over the same copy after the switch, and the answers are compared by Koha's numbers.

## 1. Behaviour

### The recorder, on the Koha side

1. `tools/koha-parity/record.pl` runs inside the Koha of the copy (`koha-shell <instance> -c 'perl record.pl ...'`), on Paulo's machine or on the oracle VM, never in the cloud. It **only reads**: it calls Koha's functions and writes nothing to the database.
2. `--now YYYY-MM-DDTHH:MM` fixes the moment of the answers, in the library's time; a space may stand for the `T`, and anything else is refused, so a date alone never passes for midnight. The default is now, cut to the minute. `--every N` keeps one loan in N, in `issue_id` order. The default, 1, keeps them all.
3. For each current loan (`issues`), it computes the due date the loan would get if it were made again at its `issuedate`. The call is `C4::Circulation::CalcDateDue(issuedate, effective item type, branch, patron)`, with the branch given by `CircControl` and `HomeOrHoldingBranch` (`C4::Circulation::_GetCircControlBranch`).
4. For each overdue loan from `C4::Overdues::Getoverdues`, at `--now`, it computes the fine with `C4::Overdues::CalcFine`, with the branch chosen as `misc/cronjobs/fines.pl` chooses it. Lost Items are skipped, as `fines.pl` skips them. This is the amount before `UpdateFine` limits it to `MaxFine`.
4a. **Renewal (part 2).** For each current loan, it asks `C4::Circulation::CanBookBeRenewed(patron, checkout)` as the staff desk asks it: without overriding the limit and outside the cron. The answer is `ok`, or Koha's reason (`too_many`, `on_reserve`, `too_soon`, `overdue`, `too_much_owing`, `onsite_checkout`, `restriction` and the others Koha has). Koha asks this at its own clock, not at `--now`, so the recorder writes that moment, cut to the minute, as `renewals_at`.
4b. **The hold of a returned Item (part 2).** For each current loan, it asks `C4::Reserves::CheckReserves(item)` with the Item as it is, which is what `AddReturn` asks when the Item comes back at its holding branch. The answer is `none`, or Koha's status (`Waiting`, `Transferred`, `Processing` or `Reserved`) with the `borrowernumber` of the hold. A patron holds a record once, so the `borrowernumber` names the hold; `reserve_id` does not come to the Elzevir.
5. **Output:** JSON on standard output. It has the Koha version, `now`, `renewals_at`, and four lists:
   - `due_dates`: `issue_id`, `itemnumber` and the due date as `YYYY-MM-DD HH:MM`;
   - `fines`: `issue_id`, `itemnumber` and the amount in cents;
   - `renewals`: `issue_id`, `itemnumber` and the answer;
   - `holds_on_return`: `issue_id`, `itemnumber` and the answer.

   No name, card number, barcode or other personal value is written: only Koha's numbers, dates and amounts.

### The comparison, on the Elzevir side

6. `elzevir parity-koha --data-dir DIR ANSWERS.json` reads the recorder's file and the database of the folder after `switch-from-koha`. The folder is locked during the read, as with every command that opens it. It writes nothing.
7. Each answer is found by its `itemnumber`, through `item_origins`, and the loan of that Item in the Elzevir. Using the Item's own loan, the Elzevir computes:
   - **Due date:** its due date from the same start (`loans.issued_at`), by the rules of the holding branch, the patron's category and the Item's policy, and by the calendar of that branch. This is the computation of `circ.checkout`.
   - **Fine:** at `now`, from the migrated due date, as the daily run computes it (`calc_fine`, without `MaxFine`).
   - **Renewal:** at `renewals_at`, the check of `circ.renew` from the staff desk, without writing. Its refusals are named with Koha's reasons: on-site loan `onsite_checkout`, no renewals left `too_many`, overdue loans `overdue`, balance over the limit `too_much_owing`, someone waiting `on_reserve`, too soon `too_soon`.
   - **Hold of the returned Item:** the hold `circ.checkin` would give the Item at its holding branch, read before anything changes, with Koha's status names: waiting `Waiting`, in transit `Transferred`, in the queue `Reserved`. The patron is named by the `borrowernumber` the switch kept.
8. **Report:** for each question, how many answers came, how many are the same, and how many could not be compared, with the reason. The reasons are: the Item did not come, the Item is not on loan in the Elzevir, or the Elzevir has no due date for it. Then comes each divergence, by `issue_id`, with both values.

   The values are dates and amounts, never personal data. The command ends with an error when there is any divergence or any answer that could not be compared, so that a script can stop on it.

## 2. Koha origin

- `C4::Circulation::CalcDateDue`, `C4::Circulation::_GetCircControlBranch`
- `C4::Overdues::Getoverdues`, `C4::Overdues::CalcFine`, `misc/cronjobs/fines.pl` (the branch of the fine)
- `C4::Circulation::CanBookBeRenewed`, `C4::Reserves::CheckReserves`

## 3. Parity

`required` (due dates, fines, renewals and holds). Proof:
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

### Second run on the oracle, part 2 (2026-11-27)

The database was backed up, and the refusal cases were built in it on purpose:
- a `norenewalbefore` of 3 days;
- two fictitious patrons, one of them owing 150.00 against a `FineNoRenewals` of 100;
- 11 loans:
  - one made today, and others made 13 days ago;
  - two overdue, by 20 and 45 days;
  - one at the limit of 5 renewals;
  - one on a record with a hold;
  - one with an item-level hold;
  - one on-site;
  - one on a record whose hold is suspended;
  - one whose record has a hold waiting with its other Item.

The database was restored afterwards. After `switch-from-koha`, `verify-koha` found no difference without a cause. `parity-koha`:
- due dates 11 of 11, and fines 2 of 2;
- renewals 10 of 11: `too_soon`, `ok`, `too_many`, `on_reserve` (record and Item), `too_much_owing` and `onsite_checkout` are all the same as Koha's;
- holds of returned Items 10 of 11: `none` and `Reserved`, including the waiting hold of the other Item, which the returned Item does not take.

The one divergence, issue 3695, is the suspended hold. The migration brings it active because the Elzevir does not suspend holds yet (a conscious deviation, [circulacao-do-koha.md](circulacao-do-koha.md), item 6). So the Elzevir refuses the renewal as `on_reserve` where Koha answers `too_soon`. It also gives the returned Item to that hold where Koha gives it to no one. This is the deviation's effect at the desk, now measured. Supporting hold suspension removes it. A third run on 2026-11-27, after hold suspension, built six suspended-hold cases on purpose (suspended only, suspended until a day with a hold behind it, an item-level suspended hold, a resumed hold, suspended until a past day, the first of the queue suspended). The switch brought the five suspended holds as suspended, `verify-koha` found no difference without a cause, and due dates 6/6, renewals 6/6 and holds of returned Items 6/6 were the same as Koha's.

## 4. Edge cases

1. A loan migrated after the dump was made, or returned in the Elzevir after the switch, is not on loan there and cannot be compared. Run the comparison right after the switch.
2. `--now` must be the same moment on both sides. The comparison uses the recorder's `now`, never its own clock.
3. A loan whose due date Koha computes differently today from what it stored (because the rules changed after the checkout) is still compared by today's computation. The stored date is the verifier's business (item 13 of [the verifier](verificacao-do-koha.md)).

4. A renewal asked at Koha's clock and at the Elzevir's minute differs only when the soonest renewal falls inside that same minute.
5. A hold whose patron did not come to the Elzevir cannot be named by `borrowernumber` and shows as a divergence, with the hold's status and no number.

## 5. Out of scope

- Renewal by the patron in the OPAC, which also refuses a restricted patron, and automatic renewal (`auto_renew`): the comparison asks as the staff desk does, and an `ok` with an automatic-renewal code counts as `ok`.
- The holds queue (`HoldsQueue`), `LocalHoldsPriority`, item groups and hold groups, which the hold check of the Elzevir leaves out (`elzevir_domain::hold::hold_for_item`).

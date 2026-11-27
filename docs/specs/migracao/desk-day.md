# A staged desk day on both systems (v0)

> Status: v0, task 6.4 of [doc 03](../../arquitetura/03-estrategia-reconstrucao.md) (Phase 6, approved by Paulo on 2026-11-27). Part a covers the circulation desk: checkout, checkin, renewal, hold and payment. Part b covers the protocols and the OPAC on the same copy, starting with SIP2 (item 11); OPAC search, Z39.50 and SRU follow. Based on: [the switch](troca-do-koha.md), [parity on the switched copy](koha-parity.md), [checkout](../circulacao/emprestimo.md), [checkin](../circulacao/devolucao.md), [renewal](../circulacao/renovacao.md), [holds](../circulacao/reservas.md), [account](../circulacao/conta.md). Goal: **the same results, or each difference listed with its cause**.

Task 6.3 asked both systems questions about the loans of a copy, without changing anything. This step **does** the work of a day at the desk on both: the same list of actions, in the same order, on the same copy, and compares what each action gave. A rule that only shows itself after an earlier action (a fine charged at checkin that then blocks a checkout, a hold placed that then blocks a renewal) is caught here.

## 1. Behaviour

### The plan

1. The plan is a JSON file with a list of steps, written by whoever runs the day. A step names patrons and Items only by Koha's numbers (`borrowernumber`, `itemnumber`), never by name, card number or barcode. The kinds of step are:
   - `checkout`: `borrowernumber`, `itemnumber`, and `confirm` (default `false`): whether the staff confirm what the check asks to confirm.
   - `checkin`: `itemnumber`.
   - `renew`: `itemnumber`.
   - `hold`: `borrowernumber`, `itemnumber`, and `level`, `record` (the default) or `item`. A record hold names its record through one of its Items.
   - `pay`: `borrowernumber` and `amount_cents`.

   At most 10,000 steps.

### The day on Koha

2. `tools/koha-parity/desk_day.pl --on-a-copy PLAN.json` runs inside the Koha of the copy (`koha-shell <instance> -c 'perl desk_day.pl ...'`), on the oracle VM or on Paulo's machine, never in the cloud. **It writes to the database**: without `--on-a-copy` it refuses to start. The copy is backed up before and restored after, as in the 6.3 runs.
3. The desk of each step is the branch where the Item is (`holdingbranch`) for `checkout`, `checkin` and `renew`, and the patron's branch for `hold` and `pay`. Each step runs at Koha's clock, which is written with the step, to the minute.
4. What each step does, with Koha's own functions:
   - `checkout`: `C4::Circulation::CanBookBeIssued`. If anything is impossible, the answer is `impossible` with the sorted keys (`DEBT`, `TOO_MANY`, `DEBARRED`, …). If something needs confirming and the step does not confirm, the answer is `confirm` with the sorted keys. Otherwise `AddIssue`, and the answer is `ok` with the due date.
   - `checkin`: `C4::Circulation::AddReturn` at the desk. The answer is `ok`, or `not_on_loan` (`NotIssued`), with the hold the Item found (`ResFound`, by status and `borrowernumber`), or `none`.
   - `renew`: `CanBookBeRenewed` as the staff desk asks it; when it allows, `AddRenewal`, and the answer is `ok` with the new due date; otherwise Koha's reason.
   - `hold`: `CanBookBeReserved` for a record hold, `CanItemBeReserved` for an item hold, with the patron's branch for pickup. When the status is `OK`, `AddReserve`, and the answer is `ok` with the hold's priority; otherwise the status.
   - `pay`: `Koha::Account::pay` of the amount, a payment at the desk. The answer is `ok`.
5. After each step it writes the **balance** of the step's patron: the total of the patron's outstanding debits (`outstanding_debits->total_outstanding`), in cents. For a checkin, the patron is the one who had the loan. A step whose numbers do not exist in the copy is answered `unknown` and changes nothing.
6. **Output:** JSON on standard output: the Koha version, and each step as it was given, with `at` (Koha's clock, `YYYY-MM-DD HH:MM`), `answer` and `balance_cents`. No name, card number or barcode is written.

### The day on the Elzevir

7. `elzevir desk-day --data-dir DIR --on-a-copy DAY.json` reads Koha's output and the folder after `switch-from-koha` of the same copy (before Koha's day). **It writes**: each step goes through the writer thread as the desk command (`circ.checkout`, `circ.checkin`, `circ.renew`, `circ.place_hold`, `circ.pay`), at the step's `at`, in the library's time zone. Without `--on-a-copy` it refuses to start.
8. The numbers are found through what the switch kept: the Item by `item_origins` (`koha.itemnumber`), the patron by `koha_keys`. The desk is the branch where the Item is, or the patron's branch, as in item 3. The patron of a checkin is the loan's patron before the checkin.
9. The answers are written with Koha's words:
   - checkout: the reasons a checkout is impossible and the confirmations, with the keys Koha gives them (the same keys the SIP2 server answers with); a checkout confirmed through goes through with `confirmed`; `ok` with the due date.
   - checkin: `ok` or `not_on_loan`, with the hold the Item answers named as in [koha-parity.md](koha-parity.md), item 7 (`Waiting`, `Transferred`, `Reserved`, with the `borrowernumber` the switch kept).
   - renew: the reasons of [koha-parity.md](koha-parity.md), item 7, or `ok` with the new due date.
   - hold: `ok` with the priority, or the status Koha gives the refusal (`tooManyReserves`, `tooManyHoldsForThisRecord`, `itemAlreadyOnHold`, `notReservable`, `debarred`, …).
   - pay: `ok`.
   - balance: what the patron owes after the step.
10. **Report:** how many steps, how many gave the same answer and the same balance, and each divergence by its position in the plan, with the kind of step and both answers. The values are keys, dates, priorities and amounts, never personal data. The command ends with an error when any step differs, so that a script can stop on it. A step that cannot be done on the Elzevir (the Item or the patron did not come) is a divergence, with that cause.

### Part b: SIP2 on the same copy

11. `tools/sip2-recorder/record.py` holds one self-check conversation (login, status, patron status and information, item information, checkout, renewal, checkin, each also with a wrong password or unknown barcode). It is recorded twice on the oracle VM: against Koha's SIP server on the copy, and against the Elzevir's `[sip2]` server on the folder switched from the same copy, with the same patron, Item and terminal login. The terminal on the Elzevir is a staff account with the circulation permission and the login of Koha's `SIPconfig.xml`; its unit is the switched branch of Koha's institution.
12. The patron's password on the Elzevir is the one the switch brought, still Koha's bcrypt hash ([Koha patrons](leitores-do-koha.md), item 12a): the SIP2 server checks it as Koha does ([sip2](../circulacao/sip2.md), item 21).
13. `tools/sip2-recorder/compare.py KOHA.json ELZEVIR.json --unit NAME=CODE --category NAME=CODE` compares the two recordings exchange by exchange. It leaves out what changes by the second (the times of day, and the checksum that covers them) and applies the approved deviations to Koha's side: the greeting and `BX` (DEV-0003, DEV-0004), and the unit and category written by name (DEV-0004). It prints how many exchanges are the same and each one that differs with both responses, and ends with an error when any differs.
14. The patron is fictitious and created on the copy for the run; its password is generated on the VM and never leaves it, nor does the SIP account's. The recordings mask the card, the barcode, the logins, the passwords and the patron's personal fields.

### Part b: OPAC search, SRU and Z39.50 on the same copy

15. `tools/koha-parity/search_compare.py QUERIES.json` asks both systems the same searches on the oracle VM, through Koha's OPAC and the Elzevir's, through SRU (CQL) and through Z39.50 (PQF). Koha's SRU and Z39.50 are its `koha-z3950-responder`, turned on for the run and off afterwards. The Elzevir's are its `[sru]` and `[z3950]` servers on the folder switched from the same copy.
16. A record is named only by Koha's `biblionumber`: Koha's OPAC gives it in the RSS feed, and both systems write it in `999$c`. No record text leaves the VM except the titles of the records that differ, and the copy's catalog has no patron data.
17. The Elzevir's OPAC total is the API's (`/api/v1/search`), the same search as the OPAC box. Its records are read through SRU with each word completed (`"word*"`), which is the same search ([search](../catalogo/busca.md), item 5a). The report checks that both totals agree.
18. For each search the tool prints both totals and the records only one side found, with up to ten numbers. When every record was read on both sides, it also prints, for the records both found through SRU and Z39.50, those whose title (`245$a`, without the final ISBD mark) or number of Items (`952`) differ. It ends with an error when any search differs.
19. Search is a "free" area (CLAUDE.md): a difference is a bug when the Elzevir misses what the library expects to find, and is otherwise recorded here with its cause.

## 2. Edge cases

1. A step that fails on both systems for the same reason agrees: two `impossible DEBT` are the same answer.
2. A checkout Koha refuses but the Elzevir makes changes the rest of the day on one side only. The comparison goes on, but every later step that touches that Item or patron may differ too; the first divergence is the one to read.
3. Koha answers a hold of an Item already on loan to the same patron differently from the desk screen (`alreadypossession`); the step answer is Koha's status as `CanItemBeReserved` gives it.
4. The day runs at Koha's real clock, minutes apart. The Elzevir replays each step at Koha's minute, not at its own clock, so a step that crosses midnight on Koha crosses it on the Elzevir too.
5. A payment larger than the balance is accepted by Koha as a credit. The balance compares only debits, as `non_issues_charges` does, so the extra credit does not show.

## 3. Koha origin

- `C4::Circulation::CanBookBeIssued`, `AddIssue`, `AddReturn`, `CanBookBeRenewed`, `AddRenewal`
- `C4::Reserves::CanBookBeReserved`, `CanItemBeReserved`, `AddReserve`
- `Koha::Account::pay`, `Koha::Account::outstanding_debits`

## 4. Parity

`required` (circulation, fines and holds). Proof:
- unit tests in `elzevir-app::desk_day`: reading the plan and Koha's output, the comparison of answers and balances, and the Koha keys of each refusal;
- a test of the binary (`crates/elzevir/tests/cli.rs`): after the switch of the synthetic dump, a day built by hand from Koha's rules for that dump matches, and a changed answer or balance is a divergence;
- `tools/tests/test_sip2_compare.py` for the SIP2 comparison, and `crates/elzevir/tests/sip2_server.rs` for a patron password still in Koha's hash;
- the day run on the oracle: a backed-up copy, the day on Koha, the switch of the copy taken before the day, the day on the Elzevir, and the restore.

### First run on the oracle (2026-11-27)

The oracle's database was backed up. Three fictitious patrons were created (category B, branch MAIN): the first with a loan 16 days overdue, the third owing 150.00 against a `noissuescharge` of 5.00. After that, the dump was taken and switched, and Koha's day ran on the database. The day had 17 steps on five Items, as follows:
- checkouts: made, refused for debt, asking to confirm another patron's hold, refused because of the fine just charged, and confirmed through;
- holds: of a record and of an Item, one of them on an Item the patron has on loan;
- renewals: refused for a hold, and of an Item no longer on loan;
- checkins: the overdue one, which charged a fine of 9.00; one that found the hold; one of an Item not on loan;
- two payments.

Afterwards the database was restored and the files were deleted.

`desk-day`: 15 of the 17 steps were the same, answers and balances. The two differences have one cause:
- Step 13: a hold for the patron owing 150.00. Koha answers `ok priority 1`, and the Elzevir refuses it with `debt_limit`.
- Step 15: a checkin of an Item that patron's hold would take. Koha answers `not_on_loan; Reserved borrowernumber 5303`, and the Elzevir answers `not_on_loan; none`, because the hold of step 13 does not exist there.

The cause is the entry point. Koha's staff desk (`reserve/placerequest.pl`) only asks `CanItemBeReserved` and `CanBookBeReserved`, which do not look at the debt. The Elzevir's desk follows the order of Koha's `POST /api/v1/holds` ([holds](../circulacao/reservas.md), item 3), which asks `Koha::Patron::can_place_holds` first: expired card, `maxoutstanding`, address. On Koha only the OPAC (`opac-reserve.pl`) and the REST API refuse a hold for debt. Paulo decided on 2026-11-27 that the desk follows Koha's staff screen. Since then the debt refuses a hold only on the OPAC and the API ([holds](../circulacao/reservas.md), item 3.3), which removes both differences.

The same day was run again on the oracle after that change, with the same setup, backup and restore. `desk-day` found 17 of 17 steps the same.

### SIP2 on the oracle (2026-11-27)

The database was backed up. A fictitious patron (category B, branch MAIN) was created with a random password, and so was the SIP account of `SIPconfig.xml`, as a staff patron. Item 1 was picked as a free Item. Then the dump was taken. Koha's conversation was recorded, the dump was switched, and the Elzevir's conversation was recorded with a staff account of the same login. The Elzevir's `[sip2]` was set by hand with the currency of Koha's `BH` (`REA`, cut from `REAL`), because the switch does not bring the active currency.

`compare.py`: 18 of the 26 exchanges were the same at first. The 8 differences had two causes:
- The copy has `ManualRenewalPeriodBase` = `now`, and the Elzevir only knew how to count from the due date. The renewal answered 2026-12-28 instead of 2026-12-11, and that changed every later due date. Since then, `[circulation] renewal_base` follows the preference, and the switch brings it ([renewal](../circulacao/renovacao.md), item 5).
- The title has accents. Koha, with no `encoding` on the login, writes Latin-1, while the Elzevir wrote UTF-8. Paulo decided on 2026-11-27 that the default follows Koha ([sip2](../circulacao/sip2.md), item 16a).

The run also showed that the SIP2 server refused a patron password still in Koha's hash (item 12). That was fixed before the comparison.

After these changes the same conversation was recorded again on the same copy, and `compare.py` found 26 of 26 exchanges the same. The database was then restored and the files were deleted.

### Search on the oracle (2026-11-27)

Koha's `koha-z3950-responder` was turned on for the run (SRU and Z39.50 on port 2100). The Elzevir served the folder switched from a dump of the same copy, with `[sru]` and `[z3950]`. Nothing in Koha's database was changed. 21 searches: 8 through the OPAC, 7 through SRU and 6 through Z39.50.

The first run found four causes, each fixed before the next run:
- Koha completes every word of the OPAC box (`QueryAutoTruncate`, on): `brasil` found 4,636 records on Koha and 1,401 on the Elzevir. Paulo decided on 2026-11-27 to do the same: `[search] complete_words`, on by default, and the switch brings Koha's value ([search](../catalogo/busca.md), item 5a).
- Series (490, 830, 440) and the place of publication (260 `$a`) were not indexed. `devorando shakespeare` found nothing on the Elzevir. Both are now words of the index ([search](../catalogo/busca.md), item 2).
- The Z39.50 server ignored truncation on word fields: `@attr 5=1 brasi` found 813 records on Koha and 16 on the Elzevir ([Z39.50](../opac/z3950.md), item 10).
- The SRU server refused `*` with diagnostic 28 ([SRU](../opac/sru.md), item 10a).

The last run: 8 of 21 searches the same, all of them exact (ISBN through the three doors, `devorando shakespeare`, `machado de assis`, `Shakespeare` as a word and as an author, title with year). `brasil` found 4,589 records on the Elzevir and 4,636 on Koha. The differences left are of the index, in an area free to improve, and are recorded here:
- **Only on Koha.** Koha's keyword index takes every field. Some are notes of the Item (`952 $z`) or of the extent (`300 $a`), and some are local fields that are no part of the description (`270`, `290`, `901`). Koha's title index also takes the statement of responsibility (`245 $c`): "Brasil" as an author's surname is a title word on Koha. On the Elzevir it is found as a contributor.
- **Only on Koha, to bring later.** Local subjects (`690`) and uncontrolled index terms (`653`) do not become subjects of the Work. A subject search for "educação" misses 6 of Koha's 490 records because of them. This goes on the list of gaps (6.6), since mapping them changes the MARC export.
- **Only on the Elzevir.** The Portuguese stemmer gives one stem to related words ("Brasília", "brasilidade" and "brasileiro" all find "brasil"), and the words of a name are matched in any order: `dc.creator="Assis, Machado de"` also finds "Machado de Assis", written in natural order in `100 $a`.

Koha's responder was then stopped and turned off, the Elzevir was stopped, and the files were deleted.

## 5. Out of this version

- Writing off, lost Items, transfers between branches and setting a found hold aside at the desk.
- Choosing the steps from the copy automatically: the plan is written by hand.

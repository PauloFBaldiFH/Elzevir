# Lost, damaged and withdrawn Items (v0)

> Status: v0, task 7.2 of [doc 03](../../arquitetura/03-estrategia-reconstrucao.md) (Phase 7, approved by Paulo on 2026-11-27). Base: [account](conta.md) (charges of a long overdue Item, items 20 to 26), [checkin](devolucao.md), [checkout](emprestimo.md), [holds](reservas.md), inventory A3 of the parity inventory. Part a: the statuses, their names and marking an Item at the desk. Part b: finding a lost Item, and what happens to its charges. Part c: the Koha migration and the proof on the oracle. Criterion: **"lose, pay, find" and "lose, find without paying" end with the same balances as in Koha**.

An Item has three statuses that take it out of circulation: **lost**, **damaged** and **withdrawn**. Each is 0 when it does not apply, or a positive number the library names (Koha keeps these names as the authorised values `LOST`, `DAMAGED` and `WITHDRAWN`; the library's copy names 6 lost values, 1 damaged and 2 withdrawn). Until now the Elzevir only read these statuses: the import and the switch bring them, the long overdue run sets lost (conta.md, item 20), and checkout, checkin, holds, the OPAC and SIP2 act on them. This task lets the staff change them, charges a patron who loses an Item on loan, and undoes the charge when the Item turns up.

## 1. Behaviour

### Statuses and marking (part a)

1. Each status of an Item is 0 or a positive number. The Item keeps the moment each status last went from 0 to a number (Koha: `itemlost_on`, `damaged_on`, `withdrawn_on`); going back to 0 clears it.
2. **Status names** are a generic registry, like the branches: for each kind (lost, damaged, withdrawn, not for loan) a number from 1 to 99 and a label of 1 to 80 characters, unique within the kind. A number without a name shows the generic name of its kind ("Lost", "Damaged", "Withdrawn", "Not for loan" in the language of the interface), so a new library can mark Items before naming anything. The registry is at `/equipe/circulacao/situacoes`, with the circulation permission; removing a name a status in use carries only brings back the generic name.
3. **Item status at the desk** is at `/equipe/circulacao/exemplar`, with the circulation permission: the staff type a barcode and see the Item's title, call number, where it is, whether it is on loan and until when (no patron data: the patron is on the patron's record), and each status with its date. Each of the three statuses is a selection of 0 and the named numbers (and the Item's current number, named or not). Saving changes one status at a time, as Koha's item page does (`catalogue/updateitem.pl`), and sends the number the page showed: when the status changed meanwhile, the change is refused and the page says so (422). An unknown barcode is refused (404). A number without a name shows as the generic name with the number, for example "Lost (2)".
4. **Marking lost** an Item on loan, from 0 to a number (Koha: `LostItem` from `moredetail`):
   - the open overdue fine of the loan stops and becomes `LOST`, as in conta.md, item 22; it is not forgiven (`WhenLostForgiveFine` off, Koha's default);
   - the patron is charged the replacement price (`LOST`) and the lost item processing fee of the circulation rules (`PROCESSING`), with the branch where the Item is (`LostChargesControl` = `ItemHomeLibrary`, `HomeOrHoldingBranch` = `holdingbranch`), never twice for the same loan, and nothing when both are 0 or empty (`WhenLostChargeReplacementFee` on, Koha's default);
   - the loan ends as returned at the branch where the Item is, and goes to the history like any return (`MarkLostItemsAsReturned` holds `moredetail`, Koha's default);
   - an open transfer of the Item is cancelled (Koha: reason `ItemLost`). Koha calls `LostItem` on every change of the lost status at the item page, so the transfer is cancelled on any such change, and a hold in transit keeps its status, as in Koha.
5. Marking lost an Item that is not on loan, or changing one lost number to another, changes only the status (and cancels the transfer, item 4): nobody is charged. The long overdue run (conta.md, item 20) also keeps the moment of the loss.
6. **Withdrawn** is always allowed, on loan or in transit (`PreventWithdrawingItemsStatus` empty, Koha's default). A withdrawn Item cannot be lent (emprestimo.md, item 9), its return is refused (devolucao.md, item 2), it fills no hold (reservas.md, edge case 7) and the OPAC shows it unavailable.
7. **Damaged** changes what the OPAC and SIP2 show (unavailable; SIP2 status `01`). As in Koha, it does not stop a checkout (`CanBookBeIssued` does not look at it), and a damaged Item can be held and fills holds (`AllowHoldsOnDamagedItems` on, Koha's default).
8. Every change goes to the `audit_log` with who made it, the Item, the status and the old and new numbers.

### Finding a lost Item (part b)

9. A lost Item is **found** when its lost status goes back to 0: at a checkin (unless `BlockReturnOfLostItems`, devolucao.md, item 5), at a checkout (emprestimo.md, item 16), at an inventory scan, or on the page of item 3. Each of these runs the same steps, once.
10. What happens to the charges follows the rules `lostreturn` and `processingreturn` of the circulation rules, by the branch of the desk that finds it (`RefundLostOnReturnControl` = `CheckinLibrary`, Koha's default; the Item's branch when there is no desk), `refund` when no rule says otherwise (Koha: `Koha::CirculationRules::get_lostreturn_policy`).
11. With `lostreturn` other than "no refund", the newest `LOST` debit of the Item not yet found is credited back with a `LOST_FOUND` credit, applied to it, and becomes `FOUND`:
    - `refund_unpaid`: the credit is what is still owed, so a paid charge gives nothing back;
    - `refund`, `restore` and `charge`: the credit is what is still owed plus what was paid on it (written off amounts are not given back), so a paid charge leaves the patron in credit;
    - a credit of 0 is not written, but the debit still becomes `FOUND`.
12. `restore`: the newest `OVERDUE` fine of the Item with status `LOST` becomes `FOUND` again; it was never forgiven, so its amount stays as it was.
13. `charge`: when the Item is found at a checkin and the newest `LOST` debit or lost fine belongs to the patron of its last loan, that loan's fine is calculated again up to the moment of the checkin (multa.md) and the fine becomes `RETURNED` (Koha: `AddReturn`, message `lost_charge`). The ended loan keeps the id of the loan it was (`returned_loans.loan_id`, migration 73) to tell its last loan; history migrated from Koha has no such id, so `charge` charges nothing for a loss from before the switch.
14. With `processingreturn` other than "no refund", the newest `PROCESSING` debit of the Item not yet found is handled as in item 11, with a `PROCESSING_FOUND` credit (`refund` or `refund_unpaid`).
15. The checkin screen, and the page of item 3, say which of these happened: the replacement refunded, the processing fee refunded, the fine restored, the fine charged.
16. Koha's limits on refunds by age (`NoRefundOnLostReturnedItemsAge`, `NoRefundOnLostFinesPaidAge`) are empty by default, and so they are not kept; credits are not applied to other debits automatically (`AccountAutoReconcile` off).

### Koha migration and proof (part c)

17. The switch brings the names of the authorised values `LOST`, `DAMAGED`, `WITHDRAWN` and `NOT_LOAN` with a number from 1 to 99 into the status names, after the catalog (`patrons.migrate_koha_statuses`). A value outside 1 to 99 (the negative not-for-loan numbers among them), or without a name, keeps its generic name; a number already named keeps its name; a name that another number of the same kind already has comes with its number, as "Perdido (6)" (Koha shows both numbers with the same name), and is left out only when that is taken too. Each Item gets `itemlost_on`, `damaged_on` and `withdrawn_on` for the statuses it has; a date whose status is 0 in the dump is left behind by an older Koha and does not come. The report gives counts only. The rules `lostreturn` and `processingreturn` come with the circulation rules (troca-do-koha.md, item 7a).
18. `verify-koha` compares the status names by kind and number (a name given with its number is the same name; one left out because that was taken too is a cause, not a difference), and the dates of each Item by `itemnumber` (an Item that did not come is a cause).
19. Two scenarios are recorded on the oracle's copy and replayed on the switched copy, as in [desk-day.md](../migracao/desk-day.md): "lose, pay, find" (checkout, mark lost, pay the charges in full, check in) and "lose, find without paying" (checkout, mark lost, check in). Each step compares the patron's balance and the account lines' types, amounts, outstanding amounts and statuses.

## 2. Edge cases

1. An Item with neither a replacement price nor a processing fee is marked lost while on loan: the loan ends, nobody is charged, and finding it does nothing to the account.
2. An Item lost, found and lost again on a new loan: the second loss charges again, because it is another loan; the second finding refunds only the newest charge.
3. An Item lost by the long overdue run (conta.md, item 20) and found: the same steps as item 9.
4. A lost Item checked out to another patron: the checkout asks for confirmation (`IssueLostItem`), and the Item is found; the refund goes to the patron who was charged.
5. A lost charge partly paid and partly written off, `refund`: the credit is what is owed plus what was paid, without the written off part.
6. The patron charged was anonymized: the debits stay with the anonymized record and are refunded the same way.
7. Withdrawing an Item on loan: the loan stays, and its return is refused while the Item is withdrawn.

## 3. Origin in Koha

- `C4::Circulation::LostItem`, `C4::Accounts::chargelostitem`, `catalogue/updateitem.pl`, `catalogue/moredetail.pl`.
- `Koha::Item::store` (the `*_on` dates, found and lost) and `Koha::Item::_set_found_trigger`.
- `Koha::CirculationRules::get_lostreturn_policy`; `C4::Circulation::AddReturn` (`WasLost`, `LostItemFeeRefunded`, `ProcessingFeeRefunded`, `LostItemFeeRestored`, `LostItemFeeCharged`), `AddIssue`, `CanBookBeIssued`.
- `C4::Reserves` (`AllowHoldsOnDamagedItems`).
- Preferences, with Koha's defaults: `WhenLostChargeReplacementFee` (1), `WhenLostForgiveFine` (0), `MarkLostItemsAsReturned` (includes `moredetail`), `RefundLostOnReturnControl` (`CheckinLibrary`), `LostChargesControl` (`ItemHomeLibrary`), `PreventWithdrawingItemsStatus` (empty), `AllowHoldsOnDamagedItems` (1), `NoRefundOnLostReturnedItemsAge` and `NoRefundOnLostFinesPaidAge` (empty), `AccountAutoReconcile` (0), `useDefaultReplacementCost` (0). The library's copy keeps all of them at the default.

## 4. Parity

`required` for the charges and refunds (items 4 and 9 to 14), proved by the scenarios of item 19; `free` for the screens and the status names.

## 5. Deviations

None so far.

## 6. Out of this version

- Claims of returned Items (`ClaimReturnedLostValue`, `return_claims`): not used by the library.
- Forgiving the fine on loss (`WhenLostForgiveFine`) and the default replacement cost by item type (`useDefaultReplacementCost`): the copy keeps them off.
- Marking many Items at once (Koha's batch item modification): Phase 9, with the batch of Items.
- Bundles (`Missing from bundle`): Koha's item bundles are not in the Elzevir.

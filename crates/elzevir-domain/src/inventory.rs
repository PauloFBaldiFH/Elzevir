// SPDX-License-Identifier: GPL-3.0-or-later
//! Inventory by reading barcodes off the shelf (docs/specs/catalogo/inventario.md).

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::catalog::ItemId;

/// What the inventory found wrong with an Item, in the order a report lists them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum InventoryProblem {
    /// Read, but no Item has the barcode.
    UnknownBarcode,
    /// Read, but the Item is withdrawn: it is not marked as seen.
    Withdrawn,
    /// Read while on loan.
    CheckedOut,
    /// Read, but outside the part of the collection being counted.
    WrongPlace,
    /// Read, and it was lost: it is lost no more.
    WasLost,
    /// Read between two Items whose call numbers it does not sort between.
    OutOfOrder,
    /// In the part being counted, but not read.
    NotScanned,
    /// In the part being counted, without a barcode to read.
    NoBarcode,
}

/// An Item that was read, as the inventory needs it. Each flag is a fact about the Item
/// that the store reads as it is; none depends on another.
#[derive(Debug, Clone, PartialEq, Eq)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "four independent facts about one Item, read from its columns"
)]
pub struct ReadItem {
    /// The Item.
    pub item: ItemId,
    /// Its call number's [`shelf_key`].
    pub sort_key: String,
    /// Whether it belongs to the part of the collection being counted.
    pub in_place: bool,
    /// Whether it is on loan.
    pub on_loan: bool,
    /// Whether it was lost.
    pub was_lost: bool,
    /// Whether it is withdrawn.
    pub withdrawn: bool,
}

/// The problems of each Item read, in reading order; an Item without one is fine.
///
/// Koha: `tools/inventory.pl` (the loop over `@scanned_items`): a withdrawn Item is only
/// reported; one on loan is reported as such and nothing else; a lost one is found. With
/// `out_of_order`, an Item whose sort key is below the one read before it, or above the
/// one read after it, is out of order; withdrawn Items do not count as neighbours.
#[must_use]
pub fn read_problems(items: &[ReadItem], out_of_order: bool) -> Vec<Vec<InventoryProblem>> {
    let shelved: Vec<usize> = (0..items.len())
        .filter(|&index| items.get(index).is_some_and(|item| !item.withdrawn))
        .collect();
    let mut problems = vec![Vec::new(); items.len()];
    for (position, &index) in shelved.iter().enumerate() {
        let (Some(item), Some(found)) = (items.get(index), problems.get_mut(index)) else {
            continue;
        };
        if out_of_order {
            let key = |at: Option<&usize>| at.and_then(|&i| items.get(i)).map(|i| &i.sort_key);
            let before = position.checked_sub(1).and_then(|p| key(shelved.get(p)));
            let after = key(shelved.get(position + 1));
            if before.is_some_and(|key| item.sort_key < *key)
                || after.is_some_and(|key| item.sort_key > *key)
            {
                found.push(InventoryProblem::OutOfOrder);
            }
        }
        if item.on_loan {
            found.push(InventoryProblem::CheckedOut);
        } else {
            if !item.in_place {
                found.push(InventoryProblem::WrongPlace);
            }
            if item.was_lost {
                found.push(InventoryProblem::WasLost);
            }
        }
        found.sort_unstable();
    }
    for (index, item) in items.iter().enumerate() {
        if let (true, Some(found)) = (item.withdrawn, problems.get_mut(index)) {
            found.push(InventoryProblem::Withdrawn);
        }
    }
    problems
}

/// The Items expected on the shelf that were not read: each with [`InventoryProblem::NoBarcode`]
/// when it has no barcode, else [`InventoryProblem::NotScanned`].
///
/// Koha: `tools/inventory.pl` with `compareinv2barcd`.
#[must_use]
pub fn unread(
    expected: &[(ItemId, bool)],
    read: &BTreeSet<ItemId>,
) -> Vec<(ItemId, InventoryProblem)> {
    expected
        .iter()
        .filter_map(|&(item, has_barcode)| {
            if !has_barcode {
                Some((item, InventoryProblem::NoBarcode))
            } else if read.contains(&item) {
                None
            } else {
                Some((item, InventoryProblem::NotScanned))
            }
        })
        .collect()
}

/// The sort key of a Dewey call number, so that call numbers compare as they stand on the
/// shelf.
///
/// Koha: `C4::ClassSortRoutine::Dewey::get_class_sort_key`.
#[must_use]
pub fn dewey_sort_key(call_number: &str) -> String {
    let upper = call_number.trim().to_uppercase().replace('/', " ");
    let upper = upper.trim();
    // An alphabetic prefix is a token of its own: "ABC123" sorts as "ABC 123".
    let prefix = upper.chars().take_while(|c| c.is_alphabetic()).count();
    let spaced: String = if prefix > 0 {
        let (head, tail): (String, String) = (
            upper.chars().take(prefix).collect(),
            upper.chars().skip(prefix).collect(),
        );
        format!("{head} {tail}")
    } else {
        upper.to_owned()
    };
    let mut tokens: Vec<String> = Vec::new();
    for part in spaced.split('.') {
        let mut pieces = part.split(char::is_whitespace).filter(|p| !p.is_empty());
        // Perl's `split /\.|\s+/` keeps an empty field between two periods, but a run of
        // whitespace is one separator.
        match pieces.next() {
            Some(first) => {
                tokens.push(first.to_owned());
                tokens.extend(pieces.map(str::to_owned));
            }
            None => tokens.push(String::new()),
        }
    }
    // Perl drops the empty fields at the end.
    while tokens.last().is_some_and(String::is_empty) {
        tokens.pop();
    }
    let digits = |token: &str| !token.is_empty() && token.bytes().all(|b| b.is_ascii_digit());
    let mut first = None;
    let mut groups = 0;
    for index in 0..tokens.len() {
        if !tokens.get(index).is_some_and(|token| digits(token)) {
            continue;
        }
        groups += 1;
        match groups {
            1 => first = Some(index),
            2 => {
                let next_to_first = first.is_some_and(|first| index - first == 1);
                if next_to_first {
                    if let Some(token) = tokens.get_mut(index) {
                        let mut padded: String = token.chars().take(15).collect();
                        while padded.chars().count() < 15 {
                            padded.push('0');
                        }
                        *token = padded;
                    }
                } else if let Some(token) = first.and_then(|first| tokens.get_mut(first)) {
                    token.push_str("_000000000000000");
                }
            }
            _ => {}
        }
    }
    if groups == 1
        && let Some(token) = first.and_then(|first| tokens.get_mut(first))
    {
        token.push_str("_000000000000000");
    }
    tokens
        .join("_")
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '_')
        .collect()
}

/// The key Items are compared by on the shelf: the Dewey sort key with its separators below
/// every letter and digit, as in Koha's database collation, where `869.1` comes before
/// `869Z`.
#[must_use]
pub fn shelf_key(call_number: &str) -> String {
    dewey_sort_key(call_number).replace('_', " ")
}

/// Whether the shelf key `key` lies in the range from `from` to `to`, both call numbers and
/// both included; with only `from`, up to `from` followed by `Z`, which takes in every call
/// number that starts like it.
///
/// Koha: `tools/inventory.pl` (`maxlocation`) and `C4::Items::GetItemsForInventory`.
#[must_use]
pub fn in_call_number_range(key: &str, from: Option<&str>, to: Option<&str>) -> bool {
    let upper = match (from, to) {
        (_, Some(to)) => Some(shelf_key(to)),
        (Some(from), None) => Some(shelf_key(&format!("{from}Z"))),
        (None, None) => None,
    };
    from.is_none_or(|from| key >= shelf_key(from).as_str())
        && upper.is_none_or(|upper| key <= upper.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dewey_keys_match_kohas_tests() {
        // reference/koha/t/ClassSortRoutine_Dewey.t, with class and item joined by a space.
        assert_eq!(dewey_sort_key(" "), "");
        assert_eq!(dewey_sort_key(".... ....."), "");
        assert_eq!(dewey_sort_key("123 456"), "123_456000000000000");
        assert_eq!(dewey_sort_key("abc123 456"), "ABC_123_456000000000000");
        assert_eq!(
            dewey_sort_key("ab         c123 45   6"),
            "AB_C123_45_600000000000000"
        );
        assert!(dewey_sort_key("YR DVD 800.1") < dewey_sort_key("YR DVD 900"));
        assert_eq!(
            dewey_sort_key("971/P23w/v.1-2/pt.8"),
            "971_000000000000000_P23W_V_12_PT_8"
        );
        assert_eq!(
            dewey_sort_key("971.01 P23w v.1-2 pt.6-7"),
            "971_010000000000000_P23W_V_12_PT_67"
        );
        assert_eq!(
            dewey_sort_key("971.01 P23w v.1-2 pt.7"),
            "971_010000000000000_P23W_V_12_PT_7"
        );
    }

    #[test]
    fn a_range_takes_in_what_starts_like_its_first_call_number() {
        let key = shelf_key("869.1 A474d");
        assert!(in_call_number_range(&key, Some("869"), None));
        assert!(in_call_number_range(&key, Some("800"), Some("899")));
        assert!(!in_call_number_range(&key, Some("870"), None));
        assert!(!in_call_number_range(&key, None, Some("860")));
        assert!(in_call_number_range(&key, None, None));
    }

    #[test]
    fn shelf_order_follows_the_dewey_number() {
        let keys: Vec<String> = ["869.3 A474d", "869.1 B123", "82-31 C"]
            .iter()
            .map(|cn| dewey_sort_key(cn))
            .collect();
        assert!(keys[1] < keys[0]);
        assert!(keys[2] < keys[1]);
    }

    fn read(item: i64, key: &str) -> ReadItem {
        ReadItem {
            item: ItemId(item),
            sort_key: shelf_key(key),
            in_place: true,
            on_loan: false,
            was_lost: false,
            withdrawn: false,
        }
    }

    #[test]
    fn each_item_read_gets_its_problems() {
        use InventoryProblem::{CheckedOut, OutOfOrder, WasLost, Withdrawn, WrongPlace};
        let mut items = vec![
            read(1, "100"),
            read(2, "300"),
            read(3, "200"),
            read(4, "400"),
            read(5, "500"),
            read(6, "050"),
            read(7, "600"),
        ];
        items[3].on_loan = true;
        items[3].in_place = false;
        items[3].was_lost = true;
        items[4].in_place = false;
        items[4].was_lost = true;
        // Withdrawn: reported, and not a neighbour for the order.
        items[5].withdrawn = true;
        let problems = read_problems(&items, true);
        assert_eq!(
            problems,
            vec![
                vec![],
                vec![OutOfOrder],
                vec![OutOfOrder],
                vec![CheckedOut],
                vec![WrongPlace, WasLost],
                vec![Withdrawn],
                vec![],
            ]
        );
        assert_eq!(read_problems(&items, false)[1], vec![]);
    }

    #[test]
    fn unread_items_are_missing_or_have_no_barcode() {
        let expected = [(ItemId(1), true), (ItemId(2), true), (ItemId(3), false)];
        assert_eq!(
            unread(&expected, &BTreeSet::from([ItemId(1)])),
            vec![
                (ItemId(2), InventoryProblem::NotScanned),
                (ItemId(3), InventoryProblem::NoBarcode),
            ]
        );
    }
}

// SPDX-License-Identifier: GPL-3.0-or-later
//! The two-character codes of the SIP2 variable fields the Elzevir reads or writes.
//!
//! Koha: `C4::SIP::Sip::Constants` (`FID_*`).

/// `AA`: the patron's card.
pub const PATRON_ID: &str = "AA";
/// `AB`: the item's barcode.
pub const ITEM_ID: &str = "AB";
/// `AC`: the terminal's password.
pub const TERMINAL_PASSWORD: &str = "AC";
/// `AD`: the patron's password.
pub const PATRON_PASSWORD: &str = "AD";
/// `AE`: the patron's name.
pub const PERSONAL_NAME: &str = "AE";
/// `AF`: a message for the screen.
pub const SCREEN_MESSAGE: &str = "AF";
/// `AG`: a line for the printer.
pub const PRINT_LINE: &str = "AG";
/// `AH`: the due date.
pub const DUE_DATE: &str = "AH";
/// `AJ`: the title.
pub const TITLE_ID: &str = "AJ";
/// `AO`: the institution.
pub const INSTITUTION_ID: &str = "AO";
/// `AP`: the current location.
pub const CURRENT_LOCATION: &str = "AP";
/// `AQ`: the permanent location.
pub const PERMANENT_LOCATION: &str = "AQ";
/// `AS`: an item on hold for the patron.
pub const HOLD_ITEMS: &str = "AS";
/// `AT`: an overdue item.
pub const OVERDUE_ITEMS: &str = "AT";
/// `AU`: an item checked out.
pub const CHARGED_ITEMS: &str = "AU";
/// `AV`: an item with a fine.
pub const FINE_ITEMS: &str = "AV";
/// `BD`: the home address.
pub const HOME_ADDRESS: &str = "BD";
/// `BE`: the e-mail address.
pub const EMAIL: &str = "BE";
/// `BF`: the home phone.
pub const HOME_PHONE: &str = "BF";
/// `BG`: the owner of the item.
pub const OWNER: &str = "BG";
/// `BH`: the currency.
pub const CURRENCY: &str = "BH";
/// `BI`: a cancelled transaction.
pub const CANCEL: &str = "BI";
/// `BK`: the transaction.
pub const TRANSACTION_ID: &str = "BK";
/// `BL`: whether the patron is known.
pub const VALID_PATRON: &str = "BL";
/// `BO`: the patron agrees to a fee.
pub const FEE_ACKNOWLEDGED: &str = "BO";
/// `BP`: the first item of a summary.
pub const START_ITEM: &str = "BP";
/// `BQ`: the last item of a summary.
pub const END_ITEM: &str = "BQ";
/// `BT`: the kind of fee.
pub const FEE_TYPE: &str = "BT";
/// `BU`: a recalled item.
pub const RECALL_ITEMS: &str = "BU";
/// `BV`: the amount owed.
pub const FEE_AMOUNT: &str = "BV";
/// `BX`: the messages the server answers.
pub const SUPPORTED_MESSAGES: &str = "BX";
/// `CC`: the most a patron may owe.
pub const FEE_LIMIT: &str = "CC";
/// `CD`: a hold not yet available.
pub const UNAVAILABLE_HOLD_ITEMS: &str = "CD";
/// `CF`: the length of the item's hold queue.
pub const HOLD_QUEUE_LENGTH: &str = "CF";
/// `CH`: the item's properties.
pub const ITEM_PROPERTIES: &str = "CH";
/// `CK`: the media type.
pub const MEDIA_TYPE: &str = "CK";
/// `CL`: the sort bin.
pub const SORT_BIN: &str = "CL";
/// `CM`: the date a hold waits until.
pub const HOLD_PICKUP_DATE: &str = "CM";
/// `CN`: the terminal's user.
pub const LOGIN_USER: &str = "CN";
/// `CO`: the terminal's password at login.
pub const LOGIN_PASSWORD: &str = "CO";
/// `CP`: the terminal's location.
pub const LOCATION_CODE: &str = "CP";
/// `CQ`: whether the patron's password is right.
pub const VALID_PATRON_PASSWORD: &str = "CQ";
/// `CR`: the collection.
pub const COLLECTION_CODE: &str = "CR";
/// `CS`: the call number.
pub const CALL_NUMBER: &str = "CS";
/// `CT`: where a returned item goes.
pub const DESTINATION_LOCATION: &str = "CT";
/// `CV`: the kind of alert.
pub const ALERT_TYPE: &str = "CV";
/// `CY`: the card of the patron a returned item is held for.
pub const HOLD_PATRON_ID: &str = "CY";
/// `DA`: the name of the patron a returned item is held for.
pub const HOLD_PATRON_NAME: &str = "DA";
/// `PB`: the birth date.
pub const PATRON_BIRTHDATE: &str = "PB";
/// `PC`: the patron's category.
pub const PATRON_CLASS: &str = "PC";
/// `PI`: the patron's internet profile.
pub const INTERNET_PROFILE: &str = "PI";

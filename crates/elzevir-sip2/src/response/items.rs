// SPDX-License-Identifier: GPL-3.0-or-later
//! The responses about items: information, checkout, checkin, renewal, and the end of
//! a patron's session.

use jiff::civil::{Date, DateTime};

use super::{CirculationStatus, Fee};
use crate::channel::Version;
use crate::codes::{
    ALERT_TYPE, CALL_NUMBER, COLLECTION_CODE, CURRENCY, CURRENT_LOCATION, DESTINATION_LOCATION,
    DUE_DATE, FEE_AMOUNT, HOLD_PATRON_ID, HOLD_PATRON_NAME, HOLD_PICKUP_DATE, HOLD_QUEUE_LENGTH,
    INSTITUTION_ID, ITEM_ID, ITEM_PROPERTIES, MEDIA_TYPE, OWNER, PATRON_ID, PERMANENT_LOCATION,
    PRINT_LINE, SCREEN_MESSAGE, SORT_BIN, TITLE_ID, VALID_PATRON,
};
use crate::text::{Writer, day, timestamp};

/// The screen message of a damaged item.
const DAMAGED: &str = "Item is damaged";

/// `18`: an item's status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemInformationResponse {
    /// Now, in the library's time zone.
    pub now: DateTime,
    /// The barcode the terminal sent.
    pub item_id: String,
    /// The item; `None` when no item has the barcode.
    pub item: Option<KnownItem>,
}

/// What the `18` says of an item found by its barcode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnownItem {
    /// The circulation status.
    pub status: CirculationStatus,
    /// Two digits: the kind of security strip.
    pub security_marker: String,
    /// Two digits: the kind of fee to use the item.
    pub fee_type: String,
    /// The barcode.
    pub id: String,
    /// The title.
    pub title: String,
    /// `CK`.
    pub media_type: Option<String>,
    /// `AQ`.
    pub permanent_location: Option<String>,
    /// `AP`.
    pub current_location: Option<String>,
    /// `CH`.
    pub properties: Option<String>,
    /// `CR`.
    pub collection: Option<String>,
    /// `BH` and `BV`: what using the item costs; `None` when nothing.
    pub fee: Option<(String, String)>,
    /// `BG`.
    pub owner: Option<String>,
    /// `CF`, written when more than zero.
    pub hold_queue_length: usize,
    /// `AH`: when it is due, cut to the minute.
    pub due: Option<DateTime>,
    /// `CM`.
    pub hold_pickup_date: Option<Date>,
    /// `AF`.
    pub screen_message: Option<String>,
    /// `AG`.
    pub print_line: Option<String>,
}

impl ItemInformationResponse {
    pub(super) fn encode(&self) -> String {
        let mut writer = Writer::new("18");
        let Some(item) = &self.item else {
            return writer
                .fixed("010101")
                .timestamp(self.now)
                .field(ITEM_ID, &self.item_id)
                .field(TITLE_ID, "")
                .finish();
        };
        writer
            .fixed(item.status.code())
            .fixed(&item.security_marker)
            .fixed(&item.fee_type)
            .timestamp(self.now);
        if item.status == CirculationStatus::Other {
            writer.field(SCREEN_MESSAGE, DAMAGED);
        }
        writer
            .field(ITEM_ID, &item.id)
            .field(TITLE_ID, &item.title)
            .maybe(MEDIA_TYPE, item.media_type.as_deref())
            .maybe(PERMANENT_LOCATION, item.permanent_location.as_deref())
            .maybe(CURRENT_LOCATION, item.current_location.as_deref())
            .maybe(ITEM_PROPERTIES, item.properties.as_deref())
            .maybe(COLLECTION_CODE, item.collection.as_deref());
        if let Some((currency, amount)) = &item.fee {
            writer.field(CURRENCY, currency).field(FEE_AMOUNT, amount);
        }
        writer.maybe(OWNER, item.owner.as_deref());
        if item.hold_queue_length > 0 {
            writer.field(HOLD_QUEUE_LENGTH, &item.hold_queue_length.to_string());
        }
        if let Some(due) = item.due {
            writer.field(DUE_DATE, &timestamp(due));
        }
        if let Some(date) = item.hold_pickup_date {
            writer.field(HOLD_PICKUP_DATE, &day(date));
        }
        writer
            .maybe(SCREEN_MESSAGE, item.screen_message.as_deref())
            .maybe(PRINT_LINE, item.print_line.as_deref())
            .finish()
    }
}

/// `12`: the answer to a loan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckoutResponse {
    /// Now, in the library's time zone.
    pub now: DateTime,
    /// The institution of the terminal.
    pub institution: String,
    /// The card the terminal sent.
    pub patron_id: String,
    /// The barcode the terminal sent.
    pub item_id: String,
    /// The loan made, or why not.
    pub outcome: Result<CheckedOut, CheckoutRefused>,
    /// What the loan costs (2.00).
    pub fee: Option<Fee>,
}

/// A loan made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckedOut {
    /// The item was already lent to this patron and the loan was renewed.
    pub renewal: bool,
    /// The item is magnetic media.
    pub magnetic_media: bool,
    /// The terminal should turn the security strip off.
    pub desensitize: bool,
    /// The title.
    pub title: String,
    /// When it is due, cut to the minute.
    pub due: Option<DateTime>,
    /// `AF`.
    pub screen_message: Option<String>,
    /// `AG`.
    pub print_line: Option<String>,
    /// `CK` (2.00).
    pub media_type: Option<String>,
    /// `CH` (2.00).
    pub properties: Option<String>,
}

/// A loan refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckoutRefused {
    /// The title, when the item exists; empty otherwise.
    pub title: String,
    /// Whether the card belongs to a patron.
    pub patron_known: bool,
    /// `AF`: why.
    pub screen_message: Option<String>,
    /// `AG`.
    pub print_line: Option<String>,
}

impl CheckoutResponse {
    pub(super) fn encode(&self, version: Version) -> String {
        let v2 = version == Version::V2;
        let mut writer = Writer::new("12");
        match &self.outcome {
            Ok(loan) => {
                writer
                    .fixed("1")
                    .yes_no(loan.renewal)
                    .yes_no(loan.magnetic_media)
                    .yes_no(loan.desensitize)
                    .timestamp(self.now);
                self.identify(&mut writer, &loan.title);
                writer
                    .field(DUE_DATE, &loan.due.map(timestamp).unwrap_or_default())
                    .maybe(SCREEN_MESSAGE, loan.screen_message.as_deref())
                    .maybe(PRINT_LINE, loan.print_line.as_deref());
                if v2 {
                    writer
                        .maybe(MEDIA_TYPE, loan.media_type.as_deref())
                        .maybe(ITEM_PROPERTIES, loan.properties.as_deref());
                }
            }
            Err(refused) => {
                writer.fixed("0NUN").timestamp(self.now);
                self.identify(&mut writer, &refused.title);
                writer
                    .field(DUE_DATE, "")
                    .maybe(SCREEN_MESSAGE, refused.screen_message.as_deref())
                    .maybe(PRINT_LINE, refused.print_line.as_deref());
                if v2 {
                    writer.yes_no_field(VALID_PATRON, refused.patron_known);
                }
            }
        }
        if v2 && let Some(fee) = &self.fee {
            fee.write(&mut writer);
        }
        writer.finish()
    }

    fn identify(&self, writer: &mut Writer, title: &str) {
        writer
            .field(INSTITUTION_ID, &self.institution)
            .field(PATRON_ID, &self.patron_id)
            .field(ITEM_ID, &self.item_id)
            .field(TITLE_ID, title);
    }
}

/// `10`: the answer to a return.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckinResponse {
    /// Now, in the library's time zone.
    pub now: DateTime,
    /// The institution of the request.
    pub institution: String,
    /// The barcode the terminal sent.
    pub item_id: String,
    /// Whether the return worked.
    pub ok: bool,
    /// The terminal should turn the security strip on.
    pub resensitize: bool,
    /// Staff should look at the item (a hold, a transfer).
    pub alert: bool,
    /// The item, when the barcode is known.
    pub item: Option<CheckinItem>,
    /// `CL` (2.00).
    pub sort_bin: Option<String>,
    /// `AA` (2.00): the card of the patron who had the item.
    pub patron_id: Option<String>,
    /// `CV`, written with an alert.
    pub alert_type: Option<String>,
    /// `AF`.
    pub screen_message: Option<String>,
    /// `AG`.
    pub print_line: Option<String>,
}

/// What the `10` says of the item returned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckinItem {
    /// The item is magnetic media.
    pub magnetic_media: bool,
    /// `AQ`.
    pub permanent_location: String,
    /// `AJ`.
    pub title: Option<String>,
    /// `CK` (2.00).
    pub media_type: Option<String>,
    /// `CH` (2.00).
    pub properties: Option<String>,
    /// `CS` (2.00).
    pub call_number: Option<String>,
    /// `CY` (2.00): the card of the patron the item is held for.
    pub hold_patron_id: Option<String>,
    /// `CT` (2.00): where the item goes.
    pub destination: Option<String>,
    /// `DA` (2.00): the name of the patron the item is held for.
    pub hold_patron_name: Option<String>,
    /// `CR` (2.00).
    pub collection: Option<String>,
}

impl CheckinResponse {
    pub(super) fn encode(&self, version: Version) -> String {
        let mut writer = Writer::new("10");
        writer
            .fixed(if self.ok { "1" } else { "0" })
            .yes_no(self.resensitize);
        match &self.item {
            Some(item) => writer.yes_no(item.magnetic_media),
            None => writer.fixed("U"),
        };
        writer
            .yes_no(self.alert)
            .timestamp(self.now)
            .field(INSTITUTION_ID, &self.institution)
            .field(ITEM_ID, &self.item_id);
        match &self.item {
            Some(item) => writer
                .field(PERMANENT_LOCATION, &item.permanent_location)
                .maybe(TITLE_ID, item.title.as_deref()),
            None => writer.field(PERMANENT_LOCATION, ""),
        };
        if version == Version::V2 {
            writer.maybe(SORT_BIN, self.sort_bin.as_deref());
            if let Some(patron) = &self.patron_id {
                writer.field(PATRON_ID, patron);
            }
            if let Some(item) = &self.item {
                write_checkin_item(&mut writer, item);
            }
        }
        if self.alert {
            writer.maybe(ALERT_TYPE, self.alert_type.as_deref());
        }
        writer
            .maybe(SCREEN_MESSAGE, self.screen_message.as_deref())
            .maybe(PRINT_LINE, self.print_line.as_deref())
            .finish()
    }
}

fn write_checkin_item(writer: &mut Writer, item: &CheckinItem) {
    writer
        .maybe(MEDIA_TYPE, item.media_type.as_deref())
        .maybe(ITEM_PROPERTIES, item.properties.as_deref())
        .maybe(CALL_NUMBER, item.call_number.as_deref())
        .maybe(HOLD_PATRON_ID, item.hold_patron_id.as_deref());
    if let Some(destination) = item.destination.as_deref().filter(|text| !text.is_empty()) {
        writer.field(DESTINATION_LOCATION, destination);
    }
    writer
        .maybe(HOLD_PATRON_NAME, item.hold_patron_name.as_deref())
        .maybe(COLLECTION_CODE, item.collection.as_deref());
}

/// `30`: the answer to a renewal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenewResponse {
    /// Now, in the library's time zone.
    pub now: DateTime,
    /// The institution of the terminal.
    pub institution: String,
    /// The renewal made, or why not.
    pub outcome: Result<Renewed, RenewRefused>,
    /// What the renewal costs.
    pub fee: Option<Fee>,
    /// `AF`.
    pub screen_message: Option<String>,
    /// `AG`.
    pub print_line: Option<String>,
}

/// A renewal made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Renewed {
    /// The patron's card.
    pub patron_id: String,
    /// The barcode.
    pub item_id: String,
    /// The title.
    pub title: String,
    /// When it is due now, cut to the minute.
    pub due: Option<DateTime>,
    /// The item is magnetic media.
    pub magnetic_media: bool,
    /// The terminal should turn the security strip off.
    pub desensitize: bool,
    /// `CK`, written even when empty.
    pub media_type: String,
    /// `CH`.
    pub properties: Option<String>,
}

/// A renewal refused: the patron, item and title found, or the ones the terminal sent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenewRefused {
    /// The card.
    pub patron_id: String,
    /// The barcode.
    pub item_id: String,
    /// The title.
    pub title: String,
}

impl RenewResponse {
    pub(super) fn encode(&self) -> String {
        let mut writer = Writer::new("30");
        match &self.outcome {
            Ok(renewed) => {
                writer
                    .fixed("1Y")
                    .yes_no(renewed.magnetic_media)
                    .yes_no(renewed.desensitize)
                    .timestamp(self.now)
                    .field(PATRON_ID, &renewed.patron_id)
                    .field(ITEM_ID, &renewed.item_id)
                    .field(TITLE_ID, &renewed.title)
                    .field(DUE_DATE, &renewed.due.map(timestamp).unwrap_or_default())
                    .field(MEDIA_TYPE, &renewed.media_type)
                    .maybe(ITEM_PROPERTIES, renewed.properties.as_deref());
            }
            Err(refused) => {
                writer
                    .fixed("0NUN")
                    .timestamp(self.now)
                    .field(PATRON_ID, &refused.patron_id)
                    .field(ITEM_ID, &refused.item_id)
                    .field(TITLE_ID, &refused.title)
                    .field(DUE_DATE, "");
            }
        }
        if let Some(fee) = &self.fee {
            fee.write(&mut writer);
        }
        writer
            .field(INSTITUTION_ID, &self.institution)
            .maybe(SCREEN_MESSAGE, self.screen_message.as_deref())
            .maybe(PRINT_LINE, self.print_line.as_deref())
            .finish()
    }
}

/// `36`: the patron's session ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EndSessionResponse {
    /// Whether it ended.
    pub ended: bool,
    /// Now, in the library's time zone.
    pub now: DateTime,
    /// The institution of the terminal.
    pub institution: String,
    /// The card the terminal sent.
    pub patron_id: String,
    /// `AF`.
    pub screen_message: Option<String>,
    /// `AG`.
    pub print_line: Option<String>,
}

impl EndSessionResponse {
    pub(super) fn encode(&self) -> String {
        Writer::new("36")
            .yes_no(self.ended)
            .timestamp(self.now)
            .field(INSTITUTION_ID, &self.institution)
            .field(PATRON_ID, &self.patron_id)
            .maybe(SCREEN_MESSAGE, self.screen_message.as_deref())
            .maybe(PRINT_LINE, self.print_line.as_deref())
            .finish()
    }
}

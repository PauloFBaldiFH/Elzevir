// SPDX-License-Identifier: GPL-3.0-or-later
//! Conversations through the SIP2 codec (docs/specs/circulacao/sip2.md, items 1 to 15),
//! with the bytes Koha's SIP server writes for the same messages.
#![expect(
    clippy::unwrap_used,
    clippy::panic,
    reason = "integration test: helpers fail the test on error"
)]

use elzevir_sip2::{
    AcsStatus, Channel, CheckedOut, CheckinItem, CheckinResponse, CheckoutRefused,
    CheckoutResponse, CirculationStatus, EndSessionResponse, Fee, ItemInformationResponse,
    KnownItem, KnownPatron, PatronCounts, PatronDetails, PatronFlag, PatronFlags,
    PatronInformationResponse, PatronStatusResponse, Received, RenewRefused, RenewResponse,
    Renewed, Request, Response, Summary, SummaryKind, Terminator, Version, checksum,
};
use jiff::civil::{DateTime, date};

const NOW: &str = "20261126    143005";

fn now() -> DateTime {
    date(2026, 11, 26).at(14, 30, 5, 0)
}

fn request(channel: &mut Channel, message: &[u8]) -> Request {
    match channel.receive(message) {
        Received::Request(request) => *request,
        Received::Reply(bytes) => panic!("answered {:?}", String::from_utf8_lossy(&bytes)),
    }
}

fn reply(channel: &mut Channel, message: &[u8]) -> String {
    match channel.receive(message) {
        Received::Reply(bytes) => String::from_utf8(bytes).unwrap(),
        Received::Request(request) => panic!("asked {request:?}"),
    }
}

fn text(bytes: Vec<u8>) -> String {
    String::from_utf8(bytes).unwrap()
}

fn with_checksum(message: &str) -> String {
    format!("{message}{:04X}", checksum(message.as_bytes()))
}

fn acs_status() -> Response {
    Response::AcsStatus(AcsStatus {
        checkin_ok: true,
        checkout_ok: true,
        renewal_ok: false,
        status_update_ok: false,
        offline_ok: false,
        timeout: 10,
        retries: 3,
        now: now(),
        institution: "Central".to_owned(),
        screen_message: None,
        print_line: None,
    })
}

#[test]
fn a_login_moves_to_2_00_and_the_acs_status_lists_the_messages_answered() {
    let mut channel = Channel::new(Terminator::CrLf);
    assert_eq!(channel.version(), Version::V1);
    let Request::Login(login) = request(&mut channel, b"9300CNterm1|COsecret|CPCentral|") else {
        panic!("not a login");
    };
    assert_eq!(
        (login.user.as_deref(), login.password.as_deref()),
        (Some("term1"), Some("secret"))
    );
    assert_eq!(channel.version(), Version::V2);
    assert_eq!(text(channel.reply(&Response::Login(true))), "941\r\n");

    assert!(matches!(
        request(&mut channel, b"9900802.00"),
        Request::ScStatus(_)
    ));
    assert_eq!(
        text(channel.reply(&acs_status())),
        format!("98YYYNNN010003{NOW}2.00AOCentral|BXYYYNYYYYYNYNNNYN|\r\n")
    );

    let mut old = Channel::new(Terminator::Cr);
    assert!(matches!(
        request(&mut old, b"9900801.00"),
        Request::ScStatus(_)
    ));
    assert_eq!(
        text(old.reply(&acs_status())),
        format!("98YYYNNN010003{NOW}1.00AOCentral|\r")
    );
    assert_eq!(
        reply(&mut old, b"6300120261126    143005          AOx|AA1|"),
        "96\r"
    );
}

#[test]
fn error_detection_checks_the_sum_and_signs_each_response() {
    let mut channel = Channel::new(Terminator::Cr);
    let login = with_checksum("9300CNterm1|COsecret|AY1AZ");
    assert!(matches!(
        request(&mut channel, login.as_bytes()),
        Request::Login(_)
    ));
    let signed = with_checksum("941AY1AZ");
    assert_eq!(
        text(channel.reply(&Response::Login(true))),
        format!("{signed}\r")
    );

    assert_eq!(
        reply(&mut channel, b"9300CNterm1|COsecret|AY2AZ0000"),
        "96AZFEF6\r"
    );
    let resend = with_checksum("97AY3AZ");
    assert_eq!(reply(&mut channel, resend.as_bytes()), "96AZFEF6\r");
    assert_eq!(reply(&mut channel, b"97AZFEF5"), "96AZFEF6\r");

    let status = with_checksum("9900802.00AY4AZ");
    assert!(matches!(
        request(&mut channel, status.as_bytes()),
        Request::ScStatus(_)
    ));
    let status_text = format!("98YYYNNN010003{NOW}2.00AOCentral|BXYYYNYYYYYNYNNNYN|");
    let signed = with_checksum(&format!("{status_text}AY4AZ"));
    assert_eq!(text(channel.reply(&acs_status())), format!("{signed}\r"));
    let again = with_checksum("97AY5AZ");
    assert_eq!(
        reply(&mut channel, b"97AZFEF5"),
        format!("{}\r", with_checksum(&format!("{status_text}AY0AZ")))
    );
    // Eleven bytes are not enough for error detection, as in Koha: the resend turns
    // it off and the last response goes again without a checksum.
    assert_eq!(
        reply(&mut channel, again.as_bytes()),
        format!("{status_text}\r")
    );
    assert_eq!(
        reply(&mut channel, b"97"),
        format!("98YYYNNN010003{NOW}2.00AOCentral|BXYYYNYYYYYNYNNNYN|\r")
    );
    assert_eq!(
        reply(&mut channel, b"97"),
        format!("98YYYNNN010003{NOW}2.00AOCentral|BXYYYNYYYYYNYNNNYN|\r")
    );
}

#[test]
fn an_empty_or_unknown_message_asks_to_send_again() {
    let mut channel = Channel::new(Terminator::CrLf);
    assert_eq!(reply(&mut channel, b"97"), "96\r\n");
    assert_eq!(reply(&mut channel, b""), "96\r");
    assert_eq!(reply(&mut channel, b"37"), "96\r");
    assert_eq!(reply(&mut channel, b"97"), "96\r");
}

fn patron() -> KnownPatron {
    KnownPatron {
        flags: PatronFlags::default().with(PatronFlag::ExcessiveFines),
        language: "000".to_owned(),
        id: "0001".to_owned(),
        name: "Teodora Lemos".to_owned(),
        password_ok: None,
        currency: Some("BRL".to_owned()),
        fee_amount: Some("12.50".to_owned()),
        screen_message: Some("Greetings from Elzevir.".to_owned()),
        print_line: None,
    }
}

#[test]
fn a_patron_status_names_the_patron_or_the_invalid_card() {
    let flags = PatronFlags::default().with(PatronFlag::ExcessiveFines);
    assert!(flags.contains(PatronFlag::ExcessiveFines));
    assert!(!flags.contains(PatronFlag::ChargeDenied));
    let status = |patron, version| {
        Response::PatronStatus(PatronStatusResponse {
            language: "001".to_owned(),
            now: now(),
            institution: "Central".to_owned(),
            patron_id: "0001".to_owned(),
            patron,
        })
        .encode(version)
    };
    assert_eq!(
        status(Some(patron()), Version::V2),
        format!(
            "24          Y   001{NOW}AETeodora Lemos|AA0001|BLY|CQN|BHBRL|BV12.50|AFGreetings from Elzevir.|AOCentral|"
        )
    );
    let wrong = KnownPatron {
        password_ok: Some(false),
        screen_message: None,
        ..patron()
    };
    assert_eq!(
        status(Some(wrong), Version::V1),
        format!("24          Y   001{NOW}AETeodora Lemos|AA0001|AF -- Invalid password|AOCentral|")
    );
    assert_eq!(
        status(None, Version::V2),
        format!("24YYYY          001{NOW}AE|AA0001|BLN|AFInvalid cardnumber|AOCentral|")
    );
}

#[test]
fn patron_information_counts_and_lists_the_items_asked_for() {
    let mut channel = Channel::new(Terminator::Cr);
    request(&mut channel, b"9300CNterm1|COsecret|");
    let Request::PatronInformation(asked) = request(
        &mut channel,
        b"63   20261126    143005  Y       AOCentral|AA0001|ADpw|",
    ) else {
        panic!("not patron information");
    };
    assert_eq!(asked.summary_kind(), Some(SummaryKind::Charged));
    let details = PatronDetails {
        counts: PatronCounts {
            charged: 2,
            overdue: 1,
            ..PatronCounts::default()
        },
        fee_limit: "5".to_owned(),
        summary: Some(Summary {
            kind: SummaryKind::Charged,
            barcodes: vec!["1001".to_owned(), "1002".to_owned()],
        }),
        email: Some("teodora@example.org".to_owned()),
        category: Some("Adulto".to_owned()),
        ..PatronDetails::default()
    };
    let information = |language: &str, patron| {
        Response::PatronInformation(PatronInformationResponse {
            language: language.to_owned(),
            now: now(),
            institution: "Central".to_owned(),
            patron_id: "0001".to_owned(),
            patron,
        })
        .encode(Version::V2)
    };
    let known = KnownPatron {
        password_ok: Some(true),
        ..patron()
    };
    assert_eq!(
        information("", Some((known, details))),
        format!(
            "64          Y   000{NOW}000000010002000000000000AOCentral|AA0001|AETeodora Lemos|BLY|CQY|BHBRL|BV12.50|CC5|AU1001|AU1002|BEteodora@example.org|PCAdulto|AFGreetings from Elzevir.|"
        )
    );
    assert_eq!(
        information("001", None),
        format!(
            "64YYYY          001{NOW}000000000000000000000000AOCentral|AA0001|AE|BLN|AFInvalid cardnumber|"
        )
    );
}

#[test]
fn item_information_gives_the_status_or_an_unknown_barcode() {
    let item = KnownItem {
        status: CirculationStatus::Charged,
        security_marker: "00".to_owned(),
        fee_type: "01".to_owned(),
        id: "1001".to_owned(),
        title: "Contos | do sertão".to_owned(),
        media_type: None,
        permanent_location: Some("Central".to_owned()),
        current_location: Some("Central".to_owned()),
        properties: None,
        collection: None,
        fee: None,
        owner: Some("Central".to_owned()),
        hold_queue_length: 1,
        due: Some(date(2026, 12, 10).at(23, 59, 0, 0)),
        hold_pickup_date: None,
        screen_message: None,
        print_line: None,
    };
    let information = |item| {
        Response::ItemInformation(ItemInformationResponse {
            now: now(),
            item_id: "1001".to_owned(),
            item,
        })
        .encode(Version::V2)
    };
    assert_eq!(
        information(Some(item.clone())),
        format!(
            "18040001{NOW}AB1001|AJContos &#124; do sertão|AQCentral|APCentral|BGCentral|CF1|AH20261210    235900|"
        )
    );
    let damaged = KnownItem {
        status: CirculationStatus::Other,
        hold_queue_length: 0,
        due: None,
        ..item
    };
    assert!(
        information(Some(damaged)).starts_with(&format!("18010001{NOW}AFItem is damaged|AB1001|"))
    );
    assert_eq!(information(None), format!("18010101{NOW}AB1001|AJ|"));
}

#[test]
fn a_checkout_answers_the_loan_or_the_refusal() {
    let checkout = |outcome, version| {
        Response::Checkout(CheckoutResponse {
            now: now(),
            institution: "Central".to_owned(),
            patron_id: "0001".to_owned(),
            item_id: "1001".to_owned(),
            outcome,
            fee: Some(Fee {
                amount: "1.00".to_owned(),
                currency: None,
                fee_type: Some("06".to_owned()),
                transaction_id: None,
            }),
        })
        .encode(version)
    };
    let loan = CheckedOut {
        renewal: false,
        magnetic_media: false,
        desensitize: true,
        title: "Contos do sertão".to_owned(),
        due: Some(date(2026, 12, 10).at(23, 59, 0, 0)),
        screen_message: None,
        print_line: None,
        media_type: Some("001".to_owned()),
        properties: None,
    };
    assert_eq!(
        checkout(Ok(loan.clone()), Version::V2),
        format!(
            "121NNY{NOW}AOCentral|AA0001|AB1001|AJContos do sertão|AH20261210    235900|CK001|BV1.00|BT06|"
        )
    );
    assert_eq!(
        checkout(Ok(loan), Version::V1),
        format!("121NNY{NOW}AOCentral|AA0001|AB1001|AJContos do sertão|AH20261210    235900|")
    );
    let refused = CheckoutRefused {
        title: String::new(),
        patron_known: true,
        screen_message: Some("Invalid Item".to_owned()),
        print_line: None,
    };
    assert_eq!(
        checkout(Err(refused), Version::V2),
        format!("120NUN{NOW}AOCentral|AA0001|AB1001|AJ|AH|AFInvalid Item|BLY|BV1.00|BT06|")
    );
}

#[test]
fn a_checkin_answers_with_the_item_and_its_hold() {
    let item = CheckinItem {
        magnetic_media: false,
        permanent_location: "Central".to_owned(),
        title: Some("Contos do sertão".to_owned()),
        media_type: None,
        properties: None,
        call_number: Some("869.3 L557c".to_owned()),
        hold_patron_id: Some("0002".to_owned()),
        destination: Some(String::new()),
        hold_patron_name: Some("Rui Prado".to_owned()),
        collection: None,
    };
    let checkin = |item, alert_type: Option<&str>| {
        Response::Checkin(CheckinResponse {
            now: now(),
            institution: "Central".to_owned(),
            item_id: "1001".to_owned(),
            ok: true,
            resensitize: true,
            alert: alert_type.is_some(),
            item,
            sort_bin: None,
            patron_id: Some("0001".to_owned()),
            alert_type: alert_type.map(str::to_owned),
            screen_message: None,
            print_line: None,
        })
        .encode(Version::V2)
    };
    assert_eq!(
        checkin(Some(item), Some("01")),
        format!(
            "101YNY{NOW}AOCentral|AB1001|AQCentral|AJContos do sertão|AA0001|CS869.3 L557c|CY0002|DARui Prado|CV01|"
        )
    );
    assert_eq!(
        checkin(None, None),
        format!("101YUN{NOW}AOCentral|AB1001|AQ|AA0001|")
    );
}

#[test]
fn a_renewal_and_the_end_of_a_session() {
    let renew = |outcome| {
        Response::Renew(RenewResponse {
            now: now(),
            institution: "Central".to_owned(),
            outcome,
            fee: None,
            screen_message: Some("Item renewed".to_owned()),
            print_line: None,
        })
        .encode(Version::V2)
    };
    let renewed = Renewed {
        patron_id: "0001".to_owned(),
        item_id: "1001".to_owned(),
        title: "Contos do sertão".to_owned(),
        due: Some(date(2026, 12, 24).at(18, 30, 0, 0)),
        magnetic_media: false,
        desensitize: true,
        media_type: String::new(),
        properties: None,
    };
    assert_eq!(
        renew(Ok(renewed)),
        format!(
            "301YNY{NOW}AA0001|AB1001|AJContos do sertão|AH20261224    183000|CK|AOCentral|AFItem renewed|"
        )
    );
    let refused = RenewRefused {
        patron_id: "0001".to_owned(),
        item_id: "1001".to_owned(),
        title: String::new(),
    };
    assert_eq!(
        renew(Err(refused)),
        format!("300NUN{NOW}AA0001|AB1001|AJ|AH|AOCentral|AFItem renewed|")
    );
    let end = Response::EndPatronSession(EndSessionResponse {
        ended: true,
        now: now(),
        institution: "Central".to_owned(),
        patron_id: "0001".to_owned(),
        screen_message: Some("Thank you!".to_owned()),
        print_line: None,
    });
    assert_eq!(
        end.encode(Version::V2),
        format!("36Y{NOW}AOCentral|AA0001|AFThank you!|")
    );
}

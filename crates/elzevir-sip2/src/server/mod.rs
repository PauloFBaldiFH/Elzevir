// SPDX-License-Identifier: GPL-3.0-or-later
//! The SIP2 server on the circulation desk (docs/specs/circulacao/sip2.md, items 16 to
//! 30): one connection, from the login to the last message.
//!
//! Koha: `C4::SIP::SIPServer::raw_transport`, `sip_protocol_loop`,
//! `C4::SIP::Sip::MsgType::handle_*`.

use std::io::{self, ErrorKind, Read, Write};
use std::sync::Arc;

use elzevir_app::loans::LoanDesk;
use elzevir_app::patron_account::PatronAccounts;
use elzevir_app::patrons::PatronDesk;
use elzevir_app::self_check::SelfCheck;
use elzevir_app::staff::{Permission, StaffDirectory, StaffIdentity, log_in};
use elzevir_domain::account::DebtSettings;
use elzevir_domain::catalog::BranchId;
use jiff::Zoned;

use crate::channel::{Channel, Received, Terminator};
use crate::frame::Framer;
use crate::request::{Login, Request};
use crate::response::{AcsStatus, EndSessionResponse, Response};

mod circulation;
mod patron;

/// A terminal allowed to sign in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Terminal {
    /// The staff login it signs in with.
    pub login: String,
    /// The branch of its desk.
    pub unit: BranchId,
    /// The branch's name: the institution of its responses.
    pub institution: String,
    /// How its responses end.
    pub terminator: Terminator,
}

/// One message a terminal sent, for the log: no passwords, cards or barcodes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SipEvent {
    /// The terminal's login, once signed in.
    pub terminal: Option<String>,
    /// The message code.
    pub message: String,
    /// What happened: `answered`, `refused`, `unavailable`, `closed`.
    pub outcome: &'static str,
}

/// What the server answers with: the circulation ports, the settings and the clock.
#[derive(Clone)]
pub struct Desk {
    /// Staff accounts, for the login.
    pub staff: Arc<dyn StaffDirectory>,
    /// Patron records.
    pub patrons: Arc<dyn PatronDesk>,
    /// Patron cards and passwords.
    pub accounts: Arc<dyn PatronAccounts>,
    /// The circulation desk.
    pub loans: Arc<dyn LoanDesk>,
    /// Items, holds and branches as a terminal reads them.
    pub self_check: Arc<dyn SelfCheck>,
    /// The library's debt settings.
    pub debt: DebtSettings,
    /// The currency of `BH`.
    pub currency: Option<String>,
    /// The terminals allowed to sign in.
    pub terminals: Vec<Terminal>,
    /// Now, in the library's time zone.
    pub clock: Arc<dyn Fn() -> Zoned + Send + Sync>,
    /// Where each message goes in the log.
    pub log: Arc<dyn Fn(SipEvent) + Send + Sync>,
}

/// The staff identity and terminal of a connection that signed in.
pub(crate) struct Session<'desk> {
    pub(crate) desk: &'desk Desk,
    pub(crate) terminal: &'desk Terminal,
    pub(crate) staff: StaffIdentity,
}

/// Storage failed: the server answers `96`.
pub(crate) struct Unavailable;

/// Serves one connection until the terminal closes it, a read times out or the
/// terminal breaks the protocol.
///
/// # Errors
///
/// When writing to the connection fails.
pub fn serve<S: Read + Write>(stream: &mut S, desk: &Desk) -> io::Result<()> {
    let mut framer = Framer::default();
    let mut channel = Channel::new(Terminator::CrLf);
    let mut session: Option<Session<'_>> = None;
    let mut buffer = [0_u8; 4096];
    loop {
        let read = match stream.read(&mut buffer) {
            Ok(0) => return Ok(()),
            Ok(read) => read,
            Err(error) if error.kind() == ErrorKind::Interrupted => continue,
            // A timeout or a reset ends the connection, as Koha's alarm does.
            Err(_) => return Ok(()),
        };
        framer.push(buffer.get(..read).unwrap_or_default());
        while let Some(message) = framer.next_message() {
            let Ok(message) = message else {
                return Ok(());
            };
            let Some(bytes) = answer(&message, &mut channel, &mut session, desk) else {
                return Ok(());
            };
            stream.write_all(&bytes)?;
            stream.flush()?;
        }
    }
}

/// The bytes that answer `message`; `None` closes the connection.
fn answer<'desk>(
    message: &[u8],
    channel: &mut Channel,
    session: &mut Option<Session<'desk>>,
    desk: &'desk Desk,
) -> Option<Vec<u8>> {
    let code = String::from_utf8_lossy(message.get(..2).unwrap_or(message)).into_owned();
    let Some(signed_in) = session.as_ref() else {
        return sign_in(message, channel, session, desk, code);
    };
    let terminal = Some(signed_in.terminal.login.clone());
    let request = match channel.receive(message) {
        Received::Reply(bytes) => return Some(bytes),
        Received::Request(request) => *request,
    };
    let (outcome, bytes) = match respond(signed_in, &request) {
        Ok(Some(response)) => ("answered", channel.reply(&response)),
        Ok(None) => ("answered", channel.ask_again()),
        Err(Unavailable) => ("unavailable", channel.ask_again()),
    };
    (desk.log)(SipEvent {
        terminal,
        message: code,
        outcome,
    });
    if let Request::Login(login) = request
        && let Some(again) = check_login(&login, desk)
    {
        *session = Some(again);
    }
    Some(bytes)
}

/// Before the login: only `93` is read; anything else closes the connection.
fn sign_in<'desk>(
    message: &[u8],
    channel: &mut Channel,
    session: &mut Option<Session<'desk>>,
    desk: &'desk Desk,
    code: String,
) -> Option<Vec<u8>> {
    if !message.starts_with(b"93") {
        (desk.log)(SipEvent {
            terminal: None,
            message: code,
            outcome: "closed",
        });
        return None;
    }
    let Received::Request(request) = channel.receive(message) else {
        // A checksum that fails before the login: Koha answers and gives up.
        return None;
    };
    let Request::Login(login) = *request else {
        return None;
    };
    let signed = check_login(&login, desk);
    let ok = signed.is_some();
    if let Some(terminal) = signed.as_ref().map(|session| session.terminal) {
        channel.set_terminator(terminal.terminator);
    }
    (desk.log)(SipEvent {
        terminal: signed
            .as_ref()
            .map(|session| session.terminal.login.clone()),
        message: code,
        outcome: if ok { "answered" } else { "refused" },
    });
    *session = signed;
    Some(channel.reply(&Response::Login(ok)))
}

/// The session of a login that names a terminal and a staff account allowed to
/// circulate; `None` when refused.
///
/// Koha: `C4::SIP::Sip::MsgType::handle_login` and `login_core`.
fn check_login<'desk>(login: &Login, desk: &'desk Desk) -> Option<Session<'desk>> {
    if login.user_algorithm != "0" && !login.user_algorithm.is_empty() {
        return None;
    }
    if login.password_algorithm != "0" && !login.password_algorithm.is_empty() {
        return None;
    }
    let user = login.user.as_deref()?;
    let terminal = desk
        .terminals
        .iter()
        .find(|terminal| terminal.login == user)?;
    let staff = log_in(
        desk.staff.as_ref(),
        user,
        login.password.as_deref().unwrap_or_default(),
    )
    .ok()?;
    staff.can(Permission::Circulation).then_some(Session {
        desk,
        terminal,
        staff,
    })
}

/// The response to a request after the login; `None` asks the terminal to send again.
fn respond(session: &Session<'_>, request: &Request) -> Result<Option<Response>, Unavailable> {
    let now = (session.desk.clock)();
    Ok(Some(match request {
        Request::Login(login) => Response::Login(check_login(login, session.desk).is_some()),
        Request::ScStatus(_) => Response::AcsStatus(acs_status(session, &now)),
        Request::Resend => return Ok(None),
        Request::PatronStatus(asked) => patron::status(session, asked, &now)?,
        Request::PatronInformation(asked) => patron::information(session, asked, &now)?,
        Request::EndPatronSession(asked) => Response::EndPatronSession(EndSessionResponse {
            ended: true,
            now: now.datetime(),
            institution: session.terminal.institution.clone(),
            patron_id: asked.patron.clone().unwrap_or_default(),
            screen_message: Some("Thank you!".to_owned()),
            print_line: None,
        }),
        Request::ItemInformation(asked) => circulation::item_information(session, asked, &now)?,
        Request::Checkout(asked) => circulation::checkout(session, asked, &now)?,
        Request::Checkin(asked) => circulation::checkin(session, asked, &now)?,
        Request::Renew(asked) => circulation::renew(session, asked, &now)?,
    }))
}

/// The policy of Koha's example `SIPconfig.xml`.
fn acs_status(session: &Session<'_>, now: &Zoned) -> AcsStatus {
    AcsStatus {
        checkin_ok: true,
        checkout_ok: true,
        renewal_ok: true,
        status_update_ok: false,
        offline_ok: false,
        timeout: 100,
        retries: 5,
        now: now.datetime(),
        institution: session.terminal.institution.clone(),
        screen_message: None,
        print_line: None,
    }
}

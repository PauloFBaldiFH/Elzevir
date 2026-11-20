// SPDX-License-Identifier: GPL-3.0-or-later
//! Sending the queued e-mail notices (docs/specs/circulacao/avisos.md, part c).
//!
//! Koha runs `misc/cronjobs/process_message_queue.pl` every 15 minutes from cron; here the
//! server sends the pending notices on its own, as often. The text is written when the
//! notice goes out, in the server's language.

use std::sync::Arc;
use std::time::Duration;

use elzevir_app::circulation_rules::{CirculationCommand, CirculationOutput};
use elzevir_app::commands::{WriteCommand, WriteOutput};
use elzevir_app::notices::{NoticeOutcome, NoticeQueue, PendingNotice};
use elzevir_store::ElzevirStore;
use elzevir_web::notice_text;
use elzevir_web_templates::I18n;
use jiff::tz::TimeZone;
use lettre::message::Mailbox;
use lettre::message::header::ContentType;
use lettre::transport::smtp::authentication::Credentials;
use lettre::{Message, SmtpTransport, Transport};

use crate::logging::{self, Event};

/// How long between runs (Koha's crontab runs the queue every 15 minutes).
const EVERY: Duration = Duration::from_mins(15);

/// Most notices one run sends.
const BATCH: usize = 100;

/// How the connection to the mail server is protected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Security {
    /// TLS from the start (port 465).
    Tls,
    /// Plain, then `STARTTLS`, which must succeed (port 587).
    StartTls,
    /// No protection, for a server on the same machine.
    None,
}

/// The mail server, from `[notices.email]` in `elzevir.toml`.
#[derive(Clone)]
pub(crate) struct MailSettings {
    /// The sender.
    pub(crate) from: Mailbox,
    /// The server's name.
    pub(crate) host: String,
    /// Its port.
    pub(crate) port: u16,
    /// How the connection is protected.
    pub(crate) security: Security,
    /// The user and the password, when the server asks for them.
    pub(crate) credentials: Option<(String, String)>,
}

impl std::fmt::Debug for MailSettings {
    // The password never goes to a log or an error message.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MailSettings")
            .field("from", &self.from)
            .field("host", &self.host)
            .field("port", &self.port)
            .field("security", &self.security)
            .field("user", &self.credentials.as_ref().map(|(user, _)| user))
            .finish_non_exhaustive()
    }
}

impl MailSettings {
    fn transport(&self) -> Result<SmtpTransport, lettre::transport::smtp::Error> {
        let builder = match self.security {
            Security::Tls => SmtpTransport::relay(&self.host)?,
            Security::StartTls => SmtpTransport::starttls_relay(&self.host)?,
            Security::None => SmtpTransport::builder_dangerous(&self.host),
        };
        let builder = builder.port(self.port);
        Ok(match &self.credentials {
            Some((user, password)) => {
                builder.credentials(Credentials::new(user.clone(), password.clone()))
            }
            None => builder,
        }
        .build())
    }
}

/// What became of one notice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Delivery {
    /// Taken or refused for good: recorded on the notice.
    Done(NoticeOutcome),
    /// The server could not be reached or refused for now: tried again next run.
    Later,
}

fn deliver(
    transport: &SmtpTransport,
    settings: &MailSettings,
    (subject, body): (String, String),
    notice: &PendingNotice,
) -> Delivery {
    let Some(Ok(to)) = notice.to.as_deref().map(str::parse::<Mailbox>) else {
        return Delivery::Done(NoticeOutcome::Failed);
    };
    let mut builder = Message::builder()
        .from(settings.from.clone())
        .to(to)
        .subject(subject)
        .header(ContentType::TEXT_PLAIN);
    if let Some(reply_to) = notice
        .reply_to
        .as_deref()
        .and_then(|address| address.parse::<Mailbox>().ok())
    {
        builder = builder.reply_to(reply_to);
    }
    let Ok(message) = builder.body(body) else {
        return Delivery::Done(NoticeOutcome::Failed);
    };
    match transport.send(&message) {
        Ok(_) => Delivery::Done(NoticeOutcome::Sent),
        Err(error) if error.is_permanent() => Delivery::Done(NoticeOutcome::Failed),
        Err(_) => Delivery::Later,
    }
}

/// Sends up to [`BATCH`] pending notices and records each outcome; how many were sent and
/// how many failed, or `None` when the queue or the server could not be reached.
fn send_pending(
    store: &ElzevirStore,
    settings: &MailSettings,
    t: &I18n,
    time_zone: &TimeZone,
) -> Option<(u64, u64)> {
    let pending = store.pending_emails(BATCH).ok()?;
    if pending.is_empty() {
        return Some((0, 0));
    }
    let transport = settings.transport().ok()?;
    let (mut sent, mut failed) = (0, 0);
    for notice in &pending {
        let outcome = match deliver(
            &transport,
            settings,
            notice_text(t, notice, time_zone),
            notice,
        ) {
            Delivery::Done(outcome) => outcome,
            Delivery::Later => return None,
        };
        let at = jiff::Timestamp::now().to_zoned(time_zone.clone());
        let marked = store
            .submit(WriteCommand::Circulation(CirculationCommand::MarkNotice {
                notice: notice.id,
                outcome,
                at,
            }))
            .map(elzevir_store::Pending::wait);
        if !matches!(
            marked,
            Ok(Ok(WriteOutput::Circulation(
                CirculationOutput::NoticeMarked
            )))
        ) {
            return None;
        }
        match outcome {
            NoticeOutcome::Sent => sent += 1,
            NoticeOutcome::Failed => failed += 1,
        }
    }
    Some((sent, failed))
}

/// Sends the pending e-mail notices now and then every 15 minutes, until `shutdown` turns
/// true.
pub(crate) async fn run(
    store: Arc<ElzevirStore>,
    (settings, t, time_zone): (MailSettings, Arc<I18n>, TimeZone),
    instance: String,
    mut shutdown: tokio::sync::watch::Receiver<bool>,
) {
    loop {
        let (store_now, settings_now, t_now, zone_now) = (
            Arc::clone(&store),
            settings.clone(),
            Arc::clone(&t),
            time_zone.clone(),
        );
        let done = tokio::task::spawn_blocking(move || {
            send_pending(&store_now, &settings_now, &t_now, &zone_now)
        })
        .await;
        match done {
            // A quiet run leaves no line in the log.
            Ok(Some((0, 0))) => {}
            Ok(Some((sent, failed))) => {
                logging::emit(&instance, Event::NoticesSent { sent, failed });
            }
            _ => logging::emit(&instance, Event::NoticesSendFailed),
        }
        tokio::select! {
            () = tokio::time::sleep(EVERY) => {}
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow() {
                    return;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use elzevir_app::notices::{NoticeAbout, NoticeId, NoticeItem};
    use elzevir_domain::notice::NoticeLevel;
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;

    fn notice() -> PendingNotice {
        PendingNotice {
            id: NoticeId(1),
            to: Some("ana@example.org".to_owned()),
            address: None,
            name: "Ana Silva".to_owned(),
            library: "Central".to_owned(),
            reply_to: Some("central@example.org".to_owned()),
            about: NoticeAbout::Overdue(NoticeLevel::Second),
            items: vec![NoticeItem {
                title: "Dom Casmurro".to_owned(),
                barcode: "0001".to_owned(),
                due: Some(
                    "2026-11-16T23:59:00-03:00[America/Sao_Paulo]"
                        .parse()
                        .unwrap(),
                ),
            }],
        }
    }

    #[test]
    fn the_notice_lists_its_items_in_the_servers_language() {
        let zone = TimeZone::get("America/Sao_Paulo").unwrap();
        let (subject, body) = notice_text(&I18n::english().unwrap(), &notice(), &zone);
        assert_eq!(subject, "Second notice: overdue items at Central");
        assert!(body.starts_with("Dear Ana Silva,\n\n"), "{body}");
        assert!(
            body.contains("\n- Dom Casmurro (0001), due 2026-11-16\n"),
            "{body}"
        );
        let (subject, body) = notice_text(&I18n::portuguese().unwrap(), &notice(), &zone);
        assert_eq!(subject, "Segundo aviso: exemplares em atraso na Central");
        assert!(
            body.contains("- Dom Casmurro (0001), venceu em 16/11/2026"),
            "{body}"
        );
    }

    #[test]
    fn the_hold_notice_says_where_and_until_when() {
        let zone = TimeZone::get("America/Sao_Paulo").unwrap();
        let mut waiting = notice();
        waiting.about = NoticeAbout::HoldWaiting(jiff::civil::date(2026, 11, 27));
        waiting.items[0].due = None;
        let (subject, body) = notice_text(&I18n::english().unwrap(), &waiting, &zone);
        assert_eq!(subject, "Your hold is waiting at Central");
        assert!(
            body.contains("- Dom Casmurro (0001)\n") && body.contains("until 2026-11-27"),
            "{body}"
        );
        let (subject, body) = notice_text(&I18n::portuguese().unwrap(), &waiting, &zone);
        assert_eq!(subject, "Sua reserva aguarda retirada na Central");
        assert!(body.contains("até 27/11/2026"), "{body}");
    }

    /// A mail server that answers one session, refusing the recipient when `refuse` is set,
    /// and hands back what it was given.
    fn mail_server(refuse: bool) -> (u16, std::thread::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut writer = stream.try_clone().unwrap();
            let mut reader = BufReader::new(stream);
            let mut seen = String::new();
            writer.write_all(b"220 localhost ESMTP\r\n").unwrap();
            let mut line = String::new();
            while reader.read_line(&mut line).unwrap() > 0 {
                let command = line.to_ascii_uppercase();
                let answer: &[u8] = if command.starts_with("EHLO") {
                    b"250-localhost\r\n250 8BITMIME\r\n"
                } else if command.starts_with("RCPT") && refuse {
                    b"550 no such mailbox\r\n"
                } else if command.starts_with("DATA") {
                    writer.write_all(b"354 go on\r\n").unwrap();
                    loop {
                        line.clear();
                        reader.read_line(&mut line).unwrap();
                        if line == ".\r\n" {
                            break;
                        }
                        seen.push_str(&line);
                    }
                    b"250 queued\r\n"
                } else if command.starts_with("QUIT") {
                    writer.write_all(b"221 bye\r\n").unwrap();
                    break;
                } else {
                    b"250 ok\r\n"
                };
                writer.write_all(answer).unwrap();
                line.clear();
            }
            seen
        });
        (port, server)
    }

    fn settings(port: u16) -> MailSettings {
        MailSettings {
            from: "Biblioteca <biblioteca@example.org>".parse().unwrap(),
            host: "127.0.0.1".to_owned(),
            port,
            security: Security::None,
            credentials: None,
        }
    }

    #[test]
    fn a_notice_goes_to_the_mail_server_with_its_reply_address() {
        let (port, server) = mail_server(false);
        let settings = settings(port);
        let transport = settings.transport().unwrap();
        let text = ("Subject".to_owned(), "Olá, Ana.".to_owned());
        assert_eq!(
            deliver(&transport, &settings, text, &notice()),
            Delivery::Done(NoticeOutcome::Sent)
        );
        drop(transport);
        let seen = server.join().unwrap();
        assert!(seen.contains("To: ana@example.org"), "{seen}");
        assert!(seen.contains("Reply-To: central@example.org"), "{seen}");
        assert!(
            seen.contains("From: Biblioteca <biblioteca@example.org>"),
            "{seen}"
        );
    }

    #[test]
    fn a_refused_address_fails_and_an_unreachable_server_waits() {
        let (port, server) = mail_server(true);
        let settings = settings(port);
        let transport = settings.transport().unwrap();
        let text = || ("Subject".to_owned(), "Body".to_owned());
        assert_eq!(
            deliver(&transport, &settings, text(), &notice()),
            Delivery::Done(NoticeOutcome::Failed)
        );
        drop(transport);
        server.join().unwrap();
        // Nobody listens on the port any more.
        let transport = settings.transport().unwrap();
        assert_eq!(
            deliver(&transport, &settings, text(), &notice()),
            Delivery::Later
        );
    }

    #[test]
    fn the_password_is_not_shown() {
        let shown = format!(
            "{:?}",
            MailSettings {
                credentials: Some(("biblioteca".to_owned(), "secret-word".to_owned())),
                ..settings(25)
            }
        );
        assert!(shown.contains("biblioteca") && !shown.contains("secret-word"));
    }
}

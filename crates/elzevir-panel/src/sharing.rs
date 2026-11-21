// SPDX-License-Identifier: GPL-3.0-or-later
//! Explicit public-catalog QR; credentials and arbitrary URLs are never encoded.
use crate::{Messages, Snapshot, TunnelState};
use ratatui::text::Line;
pub(crate) fn public_url(url: &str) -> Option<&str> {
    let host = url.strip_prefix("https://")?.strip_suffix('/')?;
    if host.is_empty()
        || host.len() > 253
        || !host
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-'))
    {
        return None;
    }
    Some(url)
}
fn qr(url: &str) -> Option<String> {
    let url = public_url(url)?;
    let code =
        qrcode::QrCode::with_error_correction_level(url.as_bytes(), qrcode::EcLevel::M).ok()?;
    Some(
        code.render::<qrcode::render::unicode::Dense1x2>()
            .quiet_zone(true)
            .build(),
    )
}
pub(crate) fn draw(
    frame: &mut ratatui::Frame,
    snapshot: Option<&Snapshot>,
    messages: &Messages,
    ascii: bool,
    notice: Option<&str>,
) {
    use ratatui::{
        layout::Rect,
        style::{Color, Style},
        widgets::{Clear, Paragraph},
    };
    let area = frame.area();
    let popup = Rect::new(
        area.x + 1,
        area.y + 1,
        area.width.saturating_sub(2),
        area.height.saturating_sub(2),
    );
    let tunnel = snapshot.and_then(|sample| {
        if let TunnelState::Configured(tunnel) = &sample.operations.tunnel {
            Some(tunnel)
        } else {
            None
        }
    });
    let code = tunnel
        .filter(|_| !ascii)
        .and_then(|tunnel| qr(&tunnel.catalog));
    let available = code.as_ref().is_some_and(|code| {
        code.lines().count() + 5 + usize::from(notice.is_some()) <= usize::from(popup.height)
            && code
                .lines()
                .all(|line| line.chars().count() + 2 <= usize::from(popup.width))
    });
    let mut rows = vec![Line::from(messages.get("broker-qr"))];
    if available {
        if let Some(code) = code {
            rows.extend(code.lines().map(|line| Line::from(line.to_owned())));
        }
    } else {
        rows.push(Line::from(messages.get(
            if tunnel.and_then(|t| public_url(&t.catalog)).is_some() {
                "address-share-url"
            } else {
                "broker-qr-unavailable"
            },
        )));
    }
    if let Some(url) = tunnel.and_then(|t| public_url(&t.catalog)) {
        rows.insert(1, Line::from(url.to_owned()));
    }
    if let Some(key) = notice {
        rows.push(Line::from(messages.get(key)));
    }
    rows.push(Line::from(messages.get("address-share-keys")));
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(rows)
            .wrap(ratatui::widgets::Wrap { trim: false })
            .style(Style::default().fg(Color::Black).bg(Color::White))
            .block(crate::view::block(messages.get("broker-qr"), ascii)),
        popup,
    );
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sharing_accepts_only_the_public_catalog_origin() {
        assert!(qr("https://catalog.example.org/").is_some());
        for url in [
            "https://user:secret@example.org/",
            "https://example.org/?token=fictional",
            "https://example.org/equipe",
            "http://example.org/",
        ] {
            assert!(qr(url).is_none());
        }
    }
}

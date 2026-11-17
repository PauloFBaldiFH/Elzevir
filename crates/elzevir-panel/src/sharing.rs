// SPDX-License-Identifier: GPL-3.0-or-later
//! Explicit public-catalog QR; credentials and arbitrary URLs are never encoded.
use crate::{Messages, Snapshot, TunnelState};
use ratatui::text::Line;
fn qr(url: &str) -> Option<String> {
    let host = url.strip_prefix("https://")?.strip_suffix('/')?;
    if host.is_empty()
        || host.len() > 253
        || !host
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-'))
    {
        return None;
    }
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
    let code = tunnel.and_then(|tunnel| qr(&tunnel.catalog));
    let available = code.as_ref().is_some_and(|code| {
        code.lines().count() + 3 <= usize::from(popup.height)
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
        rows.push(Line::from(messages.get("broker-qr-unavailable")));
    }
    if let Some(tunnel) = tunnel {
        rows.push(Line::from(crate::dashboard::text(&tunnel.catalog)));
    }
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(rows)
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

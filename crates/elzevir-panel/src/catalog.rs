// SPDX-License-Identifier: GPL-3.0-or-later
//! Local catalog intentions and bounded projections for the panel.

use crate::{
    Messages, Panel,
    ui::{Action, Scene, theme},
};
use ratatui::{
    Frame,
    layout::{Constraint, Rect},
    style::Style,
    text::Line,
    widgets::{Clear, Paragraph, Wrap},
};
use serde::{Deserialize, Serialize};

/// Catalog or circulation policy list selected by the operator.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum CatalogList {
    /// Search the library holdings. An empty query browses the collection.
    #[default]
    Records,
    /// Circulation policies maintained by the registry service.
    Policies,
}

/// Bounded catalog actions supported by the local administrative adapter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum CatalogAction {
    /// Browse or find titles using the standard catalog listing.
    Query(String),
    /// Browse circulation policies.
    Policies,
    /// Create a Work and its first Instance.
    Create(String),
    /// Edit an Instance title.
    Edit {
        /// Identifier of the Instance.
        id: i64,
        /// Version shown when beginning the edit.
        version: i64,
        /// Replacement title.
        title: String,
    },
    /// Create a circulation policy.
    CreatePolicy(String),
    /// Rename a circulation policy.
    RenamePolicy {
        /// Identifier of the policy.
        id: i64,
        /// Replacement name.
        name: String,
    },
    /// Delete a circulation policy through the registry service.
    DeletePolicy(i64),
}

impl CatalogAction {
    pub(crate) const fn key(&self) -> &'static str {
        match self {
            Self::Query(_) => "cat-search",
            Self::Policies => "cat-policies",
            Self::Create(_) | Self::CreatePolicy(_) => "cat-new",
            Self::Edit { .. } | Self::RenamePolicy { .. } => "cat-edit",
            Self::DeletePolicy(_) => "cat-delete",
        }
    }

    pub(crate) fn fields(&self) -> (Vec<&'static str>, Vec<String>) {
        let (key, value) = match self {
            Self::Query(value) => ("cat-query", value),
            Self::Create(value) | Self::Edit { title: value, .. } => ("cat-title", value),
            Self::CreatePolicy(value) | Self::RenamePolicy { name: value, .. } => {
                ("cat-policy-name", value)
            }
            Self::Policies | Self::DeletePolicy(_) => return (vec![], vec![]),
        };
        (vec![key], vec![value.clone()])
    }

    pub(crate) fn finish(mut self, values: Vec<String>) -> Option<Self> {
        match &mut self {
            Self::Query(value)
            | Self::Create(value)
            | Self::Edit { title: value, .. }
            | Self::CreatePolicy(value)
            | Self::RenamePolicy { name: value, .. } => *value = values.into_iter().next()?,
            Self::Policies | Self::DeletePolicy(_) => {}
        }
        Some(self)
    }
}

/// One page of pure presentation data.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogPage {
    /// The selected list.
    pub list: CatalogList,
    /// Query reflected in the rows.
    pub query: String,
    /// A bounded selection of matching records or policies.
    pub rows: Vec<CatalogRow>,
}

/// A visible Instance or circulation policy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogRow {
    /// Stable row identity.
    pub id: i64,
    /// The current title or policy name.
    pub title: String,
    /// Version of the Instance (zero for policies).
    pub version: i64,
    /// Hub title when present.
    pub hub: Option<String>,
    /// Related Work title.
    pub work: Option<String>,
    /// Total holdings shown by the catalog.
    pub total_items: usize,
    /// A bounded preview of holdings and their published availability.
    pub items: Vec<CatalogItem>,
}

/// A holding and its circulation state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogItem {
    /// Inventory or call number.
    pub label: String,
    /// Availability from the catalog.
    pub status: CatalogItemStatus,
}

/// The existing signals of the catalog's holdings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CatalogItemStatus {
    /// Available on the shelf.
    Available,
    /// Consultation in the library.
    InLibrary,
    /// Withdrawn, damaged or lost.
    Unavailable,
}

pub(crate) fn draw(
    frame: &mut Frame,
    panel: &Panel,
    messages: &Messages,
    work: Rect,
    context: Rect,
    ascii: bool,
    scene: &mut Scene,
) {
    let page = panel.catalog();
    let policies = page.is_some_and(|page| page.list == CatalogList::Policies);
    let heading = messages.get(if policies {
        "cat-policies"
    } else {
        "panel-catalog"
    });
    let work_block =
        crate::view::block(heading, ascii).border_style(Style::new().fg(theme::ACCENT));
    let inside = work_block.inner(work);
    frame.render_widget(work_block, work);
    let rows = page.map_or([].as_slice(), |page| page.rows.as_slice());
    let selected = panel.catalog_row.min(rows.len().saturating_sub(1));
    let list_size = usize::from(inside.height.saturating_sub(1));
    let start = selected.saturating_sub(list_size.saturating_sub(1));
    let header = if rows.is_empty() {
        "cat-empty"
    } else if policies {
        "cat-policy-column"
    } else {
        "cat-title-column"
    };
    frame.render_widget(
        Paragraph::new(messages.get(header)).style(theme::surface().fg(theme::ACCENT)),
        Rect::new(inside.x, inside.y, inside.width, inside.height.min(1)),
    );
    for (line, (index, row)) in rows
        .iter()
        .enumerate()
        .skip(start)
        .take(list_size)
        .enumerate()
    {
        let area = Rect::new(
            inside.x,
            inside.y + 1 + u16::try_from(line).unwrap_or_default(),
            inside.width,
            1,
        );
        let hover = scene.add(area, Action::CatalogRow(index));
        let style = if hover || selected == index {
            theme::selected()
        } else if index % 2 == 0 {
            theme::surface()
        } else {
            Style::new().fg(theme::TEXT).bg(theme::BACKGROUND)
        };
        let mark = match (index == selected, ascii) {
            (true, false) => "▌ ",
            (true, true) => "> ",
            _ => "  ",
        };
        frame.render_widget(
            Paragraph::new(format!("{mark}{}", crate::dashboard::text(&row.title))).style(style),
            area,
        );
    }
    if let Some(row) = rows.get(selected) {
        details(frame, context, row, messages, ascii);
    }
}

fn details(frame: &mut Frame, area: Rect, row: &CatalogRow, messages: &Messages, ascii: bool) {
    let mut lines = Vec::new();
    if let Some(hub) = &row.hub {
        lines.push(Line::from(format!("Hub: {}", crate::dashboard::text(hub))));
    }
    if let Some(work) = &row.work {
        lines.push(Line::from(format!(
            "Work: {}",
            crate::dashboard::text(work)
        )));
        lines.push(Line::from(format!(
            "Instance: {}",
            crate::dashboard::text(&row.title)
        )));
        lines.push(Line::from(format!(
            "{}: {}",
            messages.get("cat-items"),
            row.total_items
        )));
        lines.extend(row.items.iter().map(|item| {
            let (status, colour) = match item.status {
                CatalogItemStatus::Available => ("cat-available", theme::SUCCESS),
                CatalogItemStatus::InLibrary => ("cat-library-only", theme::WARNING),
                CatalogItemStatus::Unavailable => ("cat-unavailable", theme::ERROR),
            };
            Line::styled(
                format!(
                    "{}  {}",
                    crate::ui::label(messages.get(status), ascii),
                    crate::dashboard::text(&item.label)
                ),
                Style::new().fg(colour),
            )
        }));
    } else {
        lines.push(Line::from(crate::dashboard::text(&row.title)));
    }
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(crate::view::block(messages.get("panel-context"), ascii)),
        area,
    );
}

pub(crate) fn popup(
    frame: &mut Frame,
    panel: &Panel,
    messages: &Messages,
    ascii: bool,
    scene: &mut Scene,
) {
    let Some(row) = panel
        .catalog_details
        .and_then(|index| panel.catalog()?.rows.get(index))
    else {
        return;
    };
    let area = frame
        .area()
        .centered(Constraint::Length(70), Constraint::Length(13));
    scene.clear();
    crate::ui::dim(frame);
    frame.render_widget(Clear, area);
    details(frame, area, row, messages, ascii);
    crate::ui::modal_buttons(frame, area, messages, ascii, scene, None);
}

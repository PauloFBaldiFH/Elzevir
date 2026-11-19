// SPDX-License-Identifier: GPL-3.0-or-later
//! The facets strip of the staff listing (docs/specs/catalogo/facetas.md, part b): the
//! filters of a URL, the addresses that add or drop one, and each value's name.

use elzevir_app::catalog::CodeLists;
use elzevir_app::material::{CarrierKind, ContentKind};
use elzevir_app::search::{Facet, FacetCounts, Filter};
use elzevir_app::worklist::{SavedList, WorkList};
use elzevir_data::vocabulary;
use elzevir_domain::catalog::BranchId;
use elzevir_web_templates::{
    ActiveFilter, FacetMenu, FacetOption, FacetStrip, I18n, SavedListLink, WorkListLink, url_encode,
};

use crate::icons::{carrier_badge, content_badge};

/// The query parameter of a filter, written `facet:value`.
pub(crate) const FILTER_PARAM: &str = "f";

/// The filters among a URL's `f=facet:value` parameters (facetas.md, item 4).
pub(crate) fn filters_of(pairs: &[(String, String)]) -> Vec<Filter> {
    Filter::parse(
        pairs
            .iter()
            .filter(|(name, _)| name == FILTER_PARAM)
            .filter_map(|(_, value)| value.split_once(':')),
    )
}

/// The query parameter of a work list (facetas.md, item 14).
pub(crate) const LIST_PARAM: &str = "list";

/// The address of a work list's page.
pub(crate) fn list_url(list: WorkList) -> String {
    format!("/equipe/acervo?{LIST_PARAM}={}", list.name())
}

/// The work lists' buttons, `current` marked; "whole catalog" outside a list.
fn lists(current: Option<WorkList>) -> Vec<WorkListLink> {
    let whole = WorkListLink {
        label_key: "list-all",
        url: "/equipe/acervo".to_owned(),
        current: current.is_none(),
    };
    let fixed = WorkList::ALL.into_iter().filter_map(|list| {
        let label_key = match list {
            WorkList::Recent => "list-recent",
            WorkList::ImportedToday => "list-imported-today",
            // Saved lists have their own menu, not a button.
            WorkList::Saved(_) => return None,
        };
        Some(WorkListLink {
            label_key,
            url: list_url(list),
            current: current == Some(list),
        })
    });
    std::iter::once(whole).chain(fixed).collect()
}

/// The strip of a work list's page: its buttons alone (facetas.md, item 17).
pub(crate) fn list_strip(list: WorkList, empty: bool) -> FacetStrip {
    FacetStrip {
        lists: lists(Some(list)),
        list_empty: empty,
        ..FacetStrip::default()
    }
}

/// `strip` with the menu of the saved lists, `current` marked; a cataloguer may delete
/// the one shown (docs/specs/catalogo/lote.md, items 18 and 20).
pub(crate) fn with_saved(
    strip: &mut FacetStrip,
    saved: &[SavedList],
    current: Option<WorkList>,
    can_catalogue: bool,
) {
    strip.saved = saved
        .iter()
        .map(|list| SavedListLink {
            name: list.name.clone(),
            url: list_url(WorkList::Saved(list.id)),
            families: list.families,
            current: current == Some(WorkList::Saved(list.id)),
        })
        .collect();
    strip.deletable = match current {
        Some(WorkList::Saved(list)) if can_catalogue && strip.saved_current().is_some() => {
            Some(list.0)
        }
        _ => None,
    };
}

/// The listing address for `query` and `filters`.
pub(crate) fn listing_url(query: &str, filters: &[Filter]) -> String {
    let mut url = format!("/equipe/acervo?q={}", url_encode(query));
    for filter in filters {
        url.push('&');
        url.push_str(FILTER_PARAM);
        url.push('=');
        url.push_str(&url_encode(&written(filter)));
    }
    url
}

/// A filter as its URL parameter value.
fn written(filter: &Filter) -> String {
    format!("{}:{}", filter.facet.name(), filter.value)
}

/// The message key of a facet's title.
const fn title_key(facet: Facet) -> &'static str {
    match facet {
        Facet::Branch => "facet-branch",
        Facet::Content => "facet-content",
        Facet::Carrier => "facet-carrier",
        Facet::Genre => "facet-genre",
        Facet::Audience => "facet-audience",
        Facet::Language => "facet-language",
        Facet::Subject => "facet-subject",
        Facet::Year => "facet-year",
    }
}

/// A value as a person reads it: a branch's name, a content's or carrier's label, a
/// language's name; the rest as indexed.
fn label(facet: Facet, value: &str, t: &I18n, codes: &CodeLists) -> String {
    let named = match facet {
        Facet::Branch => value
            .parse()
            .ok()
            .map(|id| codes.branch(BranchId(id)).to_owned()),
        Facet::Content => {
            ContentKind::from_name(value).map(|kind| t.get(content_badge(kind).label_key))
        }
        Facet::Carrier => {
            CarrierKind::from_name(value).map(|kind| t.get(carrier_badge(kind).label_key))
        }
        Facet::Language => vocabulary::scheme("languages")
            .and_then(|scheme| scheme.concept(value))
            .map(|concept| concept.label.to_owned()),
        Facet::Genre | Facet::Audience | Facet::Subject | Facet::Year => None,
    };
    named.unwrap_or_else(|| value.to_owned())
}

/// The strip for `counts` of `query` under `filters`.
pub(crate) fn strip(
    query: &str,
    filters: &[Filter],
    counts: FacetCounts,
    t: &I18n,
    codes: &CodeLists,
) -> FacetStrip {
    let active = filters
        .iter()
        .map(|filter| {
            let rest: Vec<Filter> = filters.iter().filter(|f| *f != filter).cloned().collect();
            ActiveFilter {
                facet_key: title_key(filter.facet),
                label: label(filter.facet, &filter.value, t, codes),
                remove_url: listing_url(query, &rest),
            }
        })
        .collect();
    let full = filters.len() >= elzevir_app::search::MAX_FILTERS;
    let menus = counts
        .facets
        .into_iter()
        .map(|(facet, values)| FacetMenu {
            name: facet.name(),
            title_key: title_key(facet),
            options: values
                .into_iter()
                .map(|value| {
                    let filter = Filter {
                        facet,
                        value: value.value,
                    };
                    let add_url = (!full && !filters.contains(&filter)).then(|| {
                        let mut more = filters.to_vec();
                        more.push(filter.clone());
                        listing_url(query, &more)
                    });
                    FacetOption {
                        label: label(facet, &filter.value, t, codes),
                        families: value.families,
                        add_url,
                    }
                })
                .collect(),
        })
        .collect();
    FacetStrip {
        lists: lists(None),
        list_empty: false,
        families: Some(counts.families),
        active,
        kept: filters.iter().map(written).collect(),
        clear_url: (!filters.is_empty()).then(|| listing_url(query, &[])),
        menus,
        ..FacetStrip::default()
    }
}

#[cfg(test)]
mod tests {
    use elzevir_app::search::FacetValue;

    use super::*;

    fn pairs(raw: &[(&str, &str)]) -> Vec<(String, String)> {
        raw.iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect()
    }

    #[test]
    fn filters_come_from_f_parameters_and_go_back_into_urls() {
        let filters = filters_of(&pairs(&[
            ("q", "dom"),
            ("f", "language:por"),
            ("f", "subject:Brasil: história"),
            ("f", "nocolon"),
            ("f", "itemtype:BK"),
        ]));
        assert_eq!(filters.len(), 2);
        assert_eq!(filters[1].value, "Brasil: história");
        assert_eq!(
            listing_url("dom casmurro", &filters[..1]),
            "/equipe/acervo?q=dom%20casmurro&f=language%3Apor"
        );
    }

    #[test]
    fn the_strip_names_values_and_links_each_change() {
        let t = I18n::english().unwrap();
        let codes = CodeLists::default();
        let filters = filters_of(&pairs(&[("f", "language:por")]));
        let counts = FacetCounts {
            families: 3,
            facets: vec![(
                Facet::Language,
                vec![
                    FacetValue {
                        value: "por".to_owned(),
                        families: 3,
                    },
                    FacetValue {
                        value: "eng".to_owned(),
                        families: 1,
                    },
                ],
            )],
        };
        let strip = strip("", &filters, counts, &t, &codes);
        assert_eq!(strip.kept, ["language:por"]);
        assert_eq!(strip.active[0].label, "Portuguese");
        assert_eq!(strip.active[0].remove_url, "/equipe/acervo?q=");
        assert_eq!(strip.clear_url.as_deref(), Some("/equipe/acervo?q="));
        let options = &strip.menus[0].options;
        assert_eq!(options[0].add_url, None);
        assert_eq!(
            options[1].add_url.as_deref(),
            Some("/equipe/acervo?q=&f=language%3Apor&f=language%3Aeng")
        );
        assert_eq!(options[1].label, "English");
        let marked: Vec<_> = strip.lists.iter().map(|list| list.current).collect();
        assert_eq!(marked, [true, false, false]);
        let list = list_strip(WorkList::Recent, true);
        assert_eq!(list.lists[1].url, "/equipe/acervo?list=recent");
        assert!(list.lists[1].current && list.families.is_none() && list.menus.is_empty());
    }
}

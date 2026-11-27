// SPDX-License-Identifier: GPL-3.0-or-later
//! Printed documents typeset by Typst behind the application's
//! [`Printer`](elzevir_app::printing::Printer) port (docs/specs/catalogo/etiquetas.md).
//!
//! A template is one Typst file that reads its data, as JSON, from `sys.inputs.data`. It
//! sees no other file, no package and no clock, and only the fonts built into Typst.

#![forbid(unsafe_code)]

use std::sync::Arc;

use elzevir_app::printing::{
    CardTexts, MAX_CARDS, MAX_LABELS, PatronCard, PrintError, Printer, SpineLabel,
};
use serde_json::json;
use typst::diag::{FileError, FileResult};
use typst::foundations::{Bytes, Datetime, Dict, Duration, IntoValue};
use typst::syntax::{FileId, RootedPath, Source, VirtualPath, VirtualRoot};
use typst::text::{Font, FontBook};
use typst::utils::LazyHash;
use typst::{Library, LibraryExt, World};
use typst_layout::PagedDocument;

/// The spine label template built into the binary.
pub const SPINE_LABELS: &str = include_str!("../templates/spine-labels.typ");

/// The patron card template built into the binary.
pub const PATRON_CARDS: &str = include_str!("../templates/patron-cards.typ");

/// The library's own templates; each one absent is the built-in one.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Templates {
    /// Spine labels.
    pub spine_labels: Option<String>,
    /// Patron cards.
    pub patron_cards: Option<String>,
}

/// Typesets the documents with Typst.
#[derive(Clone)]
pub struct TypstPrinter {
    spine_labels: Arc<str>,
    patron_cards: Arc<str>,
    fonts: Arc<Fonts>,
}

impl TypstPrinter {
    /// A printer with the library's templates, else the built-in ones.
    #[must_use]
    pub fn new(templates: Templates) -> Self {
        let or = |own: Option<String>, built_in: &str| {
            own.map_or_else(|| Arc::from(built_in), Arc::from)
        };
        Self {
            spine_labels: or(templates.spine_labels, SPINE_LABELS),
            patron_cards: or(templates.patron_cards, PATRON_CARDS),
            fonts: Arc::new(Fonts::embedded()),
        }
    }

    /// The PDF of `template` with `data`.
    fn render(&self, template: &str, data: &serde_json::Value) -> Result<Vec<u8>, PrintError> {
        let world = Document::new(template, data, Arc::clone(&self.fonts))?;
        let document: PagedDocument = typst::compile(&world)
            .output
            .map_err(|errors| PrintError::Template(messages(&errors)))?;
        typst_pdf::pdf(&document, &typst_pdf::PdfOptions::default())
            .map_err(|errors| PrintError::Template(messages(&errors)))
    }
}

impl Printer for TypstPrinter {
    fn spine_labels(&self, labels: &[SpineLabel]) -> Result<Vec<u8>, PrintError> {
        if labels.len() > MAX_LABELS {
            return Err(PrintError::TooMany {
                count: labels.len(),
                max: MAX_LABELS,
            });
        }
        self.render(&self.spine_labels, &json!({ "labels": labels }))
    }

    fn patron_cards(&self, cards: &[PatronCard], texts: &CardTexts) -> Result<Vec<u8>, PrintError> {
        if cards.len() > MAX_CARDS {
            return Err(PrintError::TooMany {
                count: cards.len(),
                max: MAX_CARDS,
            });
        }
        self.render(
            &self.patron_cards,
            &json!({ "cards": cards, "texts": texts }),
        )
    }
}

impl Default for TypstPrinter {
    /// The printer with the built-in templates.
    fn default() -> Self {
        Self::new(Templates::default())
    }
}

/// The diagnostics of a failed run, one per line.
fn messages(errors: &[typst::diag::SourceDiagnostic]) -> String {
    errors
        .iter()
        .map(|error| error.message.as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

/// The fonts built into Typst, read once.
struct Fonts {
    book: LazyHash<FontBook>,
    fonts: Vec<Font>,
}

impl Fonts {
    fn embedded() -> Self {
        let fonts: Vec<Font> = typst_assets::fonts()
            .flat_map(|data| Font::iter(Bytes::new(data)))
            .collect();
        Self {
            book: LazyHash::new(FontBook::from_fonts(&fonts)),
            fonts,
        }
    }
}

/// One document to typeset: its template, its data and the fonts.
struct Document {
    library: LazyHash<Library>,
    main: Source,
    fonts: Arc<Fonts>,
}

impl Document {
    fn new(
        template: &str,
        data: &serde_json::Value,
        fonts: Arc<Fonts>,
    ) -> Result<Self, PrintError> {
        let mut inputs = Dict::new();
        inputs.insert("data".into(), data.to_string().into_value());
        let path = VirtualPath::new("/main.typ")
            .map_err(|error| PrintError::Template(format!("{error:?}")))?;
        let main = RootedPath::new(VirtualRoot::Project, path).intern();
        Ok(Self {
            library: LazyHash::new(Library::builder().with_inputs(inputs).build()),
            main: Source::new(main, template.to_owned()),
            fonts,
        })
    }
}

impl World for Document {
    fn library(&self) -> &LazyHash<Library> {
        &self.library
    }

    fn book(&self) -> &LazyHash<FontBook> {
        &self.fonts.book
    }

    fn main(&self) -> FileId {
        self.main.id()
    }

    fn source(&self, id: FileId) -> FileResult<Source> {
        if id == self.main.id() {
            Ok(self.main.clone())
        } else {
            Err(FileError::AccessDenied)
        }
    }

    fn file(&self, id: FileId) -> FileResult<Bytes> {
        if id == self.main.id() {
            Ok(Bytes::from_string(self.main.text().to_owned()))
        } else {
            Err(FileError::AccessDenied)
        }
    }

    fn font(&self, index: usize) -> Option<Font> {
        self.fonts.fonts.get(index).cloned()
    }

    fn today(&self, _offset: Option<Duration>) -> Option<Datetime> {
        // Documents do not depend on the clock.
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn label(call_number: &str) -> SpineLabel {
        SpineLabel {
            call_number: call_number.to_owned(),
            lines: elzevir_domain::labels::split_dewey_call_number(call_number),
            barcode: Some("0001".to_owned()),
            title: "Dom Casmurro".to_owned(),
            author: None,
        }
    }

    #[test]
    fn spine_labels_are_typeset_as_a_pdf_a_page_per_sheet() {
        let printer = TypstPrinter::default();
        let labels: Vec<SpineLabel> = (0..70).map(|_| label("869.3 A848d ex.1")).collect();
        let pdf = printer.spine_labels(&labels).unwrap();
        assert!(pdf.starts_with(b"%PDF-"));
        // 65 labels to a sheet: two pages.
        let text = String::from_utf8_lossy(&pdf);
        assert!(text.contains("/Count 2"), "{} bytes", pdf.len());
    }

    #[test]
    fn a_broken_template_says_why() {
        let printer = TypstPrinter::new(Templates {
            spine_labels: Some("#let x = (".to_owned()),
            ..Templates::default()
        });
        let refused = printer.spine_labels(&[label("869.3 A848d")]);
        assert!(matches!(refused, Err(PrintError::Template(message)) if !message.is_empty()));
    }

    #[test]
    fn a_template_reaches_no_file() {
        let printer = TypstPrinter::new(Templates {
            spine_labels: Some("#read(\"/etc/passwd\")".to_owned()),
            ..Templates::default()
        });
        assert!(matches!(
            printer.spine_labels(&[]),
            Err(PrintError::Template(_))
        ));
    }

    #[test]
    fn too_many_labels_are_refused() {
        let labels = vec![label("869.3"); MAX_LABELS + 1];
        assert_eq!(
            TypstPrinter::default().spine_labels(&labels),
            Err(PrintError::TooMany {
                count: MAX_LABELS + 1,
                max: MAX_LABELS
            })
        );
    }

    fn card(cardnumber: &str) -> PatronCard {
        PatronCard {
            name: "Ana Silva".to_owned(),
            surname: "Silva".to_owned(),
            first_name: Some("Ana".to_owned()),
            cardnumber: cardnumber.to_owned(),
            category: "Adulto".to_owned(),
            branch: "Biblioteca Central".to_owned(),
            expires: Some("19/11/2027".to_owned()),
            bars: elzevir_domain::labels::code39(cardnumber),
        }
    }

    #[test]
    fn patron_cards_are_typeset_ten_to_a_sheet() {
        let texts = CardTexts {
            cardnumber: "Cartão".to_owned(),
            expires: "Validade".to_owned(),
        };
        let mut cards: Vec<PatronCard> = (0..10).map(|n| card(&format!("00012{n}"))).collect();
        cards.push(card("sem-código"));
        let pdf = TypstPrinter::default()
            .patron_cards(&cards, &texts)
            .unwrap();
        assert!(pdf.starts_with(b"%PDF-"));
        assert!(String::from_utf8_lossy(&pdf).contains("/Count 2"));
        let many = vec![card("1"); MAX_CARDS + 1];
        assert!(matches!(
            TypstPrinter::default().patron_cards(&many, &texts),
            Err(PrintError::TooMany { .. })
        ));
    }
}

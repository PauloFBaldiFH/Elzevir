// Elzevir patron cards (docs/specs/circulacao/carteirinhas.md).
//
// To change the card, copy this file into the data directory, edit it and name the copy
// in elzevir.toml:
//
//   [printing]
//   patron_cards = "modelos/carteirinhas.typ"
//
// Each card of `cards` has `name`, `surname`, `first_name`, `cardnumber`, `category`,
// `branch`, `expires` and `bars`; `first_name`, `expires` and `bars` may be `none`.
// `bars` is the card number in Code 39: widths in narrow elements, bar first.
// `texts` has the words around the data in the interface's language.

// An A4 sheet of 10 cards of 85.6 x 54 mm (ID-1), in 2 columns and 5 rows, with a thin
// border to cut along.
#let sheet = (
  width: 210mm,
  height: 297mm,
  top: 7.5mm,
  left: 14.4mm,
  columns: 2,
  rows: 5,
  card-width: 85.6mm,
  card-height: 54mm,
  column-gap: 10mm,
  row-gap: 3mm,
)
// Width of a narrow bar, and the most the barcode may take.
#let narrow = 0.33mm
#let barcode-width = 75mm
#let barcode-height = 11mm

#let data = json(bytes(sys.inputs.data))

#set page(
  width: sheet.width,
  height: sheet.height,
  margin: (top: sheet.top, left: sheet.left, rest: 0mm),
)
#set text(font: "Libertinus Serif", size: 10pt)

#let barcode(widths) = {
  let total = widths.sum()
  let module = if total * narrow > barcode-width { barcode-width / total } else { narrow }
  stack(
    dir: ltr,
    ..widths.enumerate().map(((index, width)) => if calc.even(index) {
      rect(width: width * module, height: barcode-height, fill: black, stroke: none)
    } else {
      width * module
    }),
  )
}

#let card(entry) = box(
  width: 100%,
  height: 100%,
  stroke: 0.3pt + luma(160),
  radius: 3mm,
  inset: 4mm,
  clip: true,
  {
    text(size: 8pt, weight: "bold", upper(entry.branch))
    v(1.5mm)
    text(size: 13pt, weight: "bold", entry.name)
    linebreak()
    text(size: 9pt, entry.category)
    if entry.expires != none {
      h(1fr)
      text(size: 9pt, data.texts.expires + " " + entry.expires)
    }
    v(1fr)
    align(center, {
      if entry.bars != none { barcode(entry.bars) }
      text(font: "DejaVu Sans Mono", size: 9pt, data.texts.cardnumber + " " + entry.cardnumber)
    })
  },
)

#for (page, chunk) in data.cards.chunks(sheet.columns * sheet.rows).enumerate() {
  if page > 0 { pagebreak() }
  grid(
    columns: (sheet.card-width,) * sheet.columns,
    rows: (sheet.card-height,) * sheet.rows,
    column-gutter: sheet.column-gap,
    row-gutter: sheet.row-gap,
    ..chunk.map(card),
  )
}

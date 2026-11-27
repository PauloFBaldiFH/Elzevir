// Elzevir spine labels (docs/specs/catalogo/etiquetas.md).
//
// To fit another sheet, copy this file into the data directory, change the measures
// below and name the copy in elzevir.toml:
//
//   [printing]
//   spine_labels = "modelos/etiquetas.typ"
//
// Each label of `labels` has `call_number`, `lines` (the call number split for the
// spine), `barcode`, `title` and `author`; the last three may be `none`.

// An A4 sheet of 65 labels of 38.1 x 21.2 mm, in 5 columns and 13 rows.
#let sheet = (
  width: 210mm,
  height: 297mm,
  top: 10.7mm,
  left: 4.7mm,
  columns: 5,
  rows: 13,
  label-width: 38.1mm,
  label-height: 21.2mm,
  column-gap: 2.5mm,
  row-gap: 0mm,
)

#let labels = json(bytes(sys.inputs.data)).labels

#set page(
  width: sheet.width,
  height: sheet.height,
  margin: (top: sheet.top, left: sheet.left, rest: 0mm),
)
#set text(font: "DejaVu Sans Mono", size: 9pt, weight: "bold")
#set par(leading: 0.35em)

#let spine(entry) = box(
  width: 100%,
  height: 100%,
  clip: true,
  align(center + horizon, entry.lines.join(linebreak())),
)

#for (page, chunk) in labels.chunks(sheet.columns * sheet.rows).enumerate() {
  if page > 0 { pagebreak() }
  grid(
    columns: (sheet.label-width,) * sheet.columns,
    rows: (sheet.label-height,) * sheet.rows,
    column-gutter: sheet.column-gap,
    row-gutter: sheet.row-gap,
    ..chunk.map(spine),
  )
}

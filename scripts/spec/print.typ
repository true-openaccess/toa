// Pandoc template for the PDF of the TOA standard (pandoc -t typst, then typst compile).
// Adapted from the PDF template of rich-netlist/rnl (MIT).
// Variables: title, revision, typst-lang, contents-title, and the font lists serif, sans and mono,
// which build.py fills with the families that typst finds on the machine.

#let horizontalrule = line(start: (25%, 0%), end: (75%, 0%))
#show terms: it => {
  it.children
    .map(child => [
      #strong[#child.term]
      #block(inset: (left: 1.5em, top: -0.4em))[#child.description]
    ])
    .join()
}
#set smartquote(enabled: false)

#let serif = ($for(serif)$"$serif$", $endfor$)
#let sans = ($for(sans)$"$sans$", $endfor$)
#let mono = ($for(mono)$"$mono$", $endfor$)

#set document(title: [$title$])
#set text(font: serif, size: 10pt, lang: "$typst-lang$")
#set par(justify: true, leading: 0.7em, spacing: 1em)
#set page(
  paper: "a4",
  margin: (x: 2.3cm, top: 2.6cm, bottom: 2.4cm),
  header: context {
    if counter(page).get().first() > 1 {
      set text(font: sans, size: 7.5pt, fill: luma(110))
      [$title$ #h(1fr) $revision$]
      v(-4pt)
      line(length: 100%, stroke: 0.4pt + luma(200))
    }
  },
  footer: context {
    if counter(page).get().first() > 1 {
      set text(font: sans, size: 8pt)
      h(1fr)
      counter(page).display()
      h(1fr)
    }
  },
)
#show raw: set text(font: mono, size: 0.88em)
#show raw.where(block: false): box.with(fill: luma(242), inset: (x: 2pt), outset: (y: 2.5pt), radius: 2pt)
#show raw.where(block: true): block.with(fill: luma(246), inset: 7pt, radius: 3pt, width: 100%, stroke: 0.5pt + luma(215))
#show heading: set text(font: sans)
#show heading: set block(above: 1.5em, below: 0.75em)
#show heading.where(level: 2): it => {
  pagebreak(weak: true)
  set text(size: 16pt)
  block(below: 1em, it.body)
}
#show heading.where(level: 3): set text(size: 12pt)
#show heading.where(level: 4): set text(size: 10.5pt)
#show link: set text(fill: rgb("#0b57d0"))
#set table(inset: 4.5pt, stroke: 0.4pt + luma(200))
#show table: set text(size: 8.6pt)
#show table: set par(justify: false)
#show table.cell: set align(start)
#show figure: set block(breakable: true)
#show outline.entry.where(level: 2): set text(font: sans, weight: "bold")

#page(header: none, footer: none)[
  #v(28%)
  #text(font: sans, size: 24pt, weight: "bold")[$title$]
  #v(0.8em)
  #text(font: sans, size: 11pt, fill: luma(80))[$revision$]
]

#outline(title: [$contents-title$], depth: 3, indent: auto)
#pagebreak()

$body$

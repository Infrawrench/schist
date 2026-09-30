# IDML fixtures

Sample `.idml` documents used by `crates/codec-idml`'s tests, and where
they came from.

## Provenance

Downloaded from **Customer's Canvas Hub**, which publishes these documents
for testing the InDesign importer of its Design Editor:

- gallery: <https://customerscanvas.com/docs/hub/designers-manual/adobe/indesign/gallery/>
- files: <https://customerscanvas.com/docs/hub/files/>

They are offered publicly, described as sample IDML files illustrating
features the product supports, and are the vendor's own exports from
InDesign.

`idml-images.zip` was a bundle of the images sample plus its `Links/`
folder; `images.idml` is the document extracted from it.

## Why this is within the project's rules

`AGENTS.md` forbids reading Adobe header files and decompiling Adobe
binaries. None of that happened, and none of it was needed:

- IDML is a **published specification** (the IDML File Format
  Specification, with RNC schemas per part), not a proprietary format.
- Every file here is a ZIP of **plain XML**. Nothing was disassembled,
  decompiled, or reverse engineered from a binary.
- The files were obtained from a **third party that publishes them for
  this purpose**, not from Adobe, and not from an installation of
  InDesign.

The same reasoning as `docs/affinity-format.md`, with one important
difference in our favour: Affinity publishes no spec, so that work was
observation of real files. IDML has a spec, so this work is checking an
implementation against a document *and* against files produced by someone
else.

## What each file covers

| File | Covers |
| --- | --- |
| `text.idml` | text placeholders, text on a path, stroke and shadow effects, limited line and character counts |
| `bounded-text.idml` | wrapping text, multi-column frames, bounded-text alignment, OpenType features |
| `shapes.idml` | vector shapes, fills, strokes |
| `multipage.idml` | several pages, page geometry, page numbering |
| `themes.idml` | colour swatches, colour groups, themes, fonts |
| `placeholders.idml` | placeholder frames, barcodes, an oval image frame with cubic clipping — **no stories** |
| `images.idml` | linked and embedded images, a restricted source folder — **no stories** |

## What these files already taught us

They are kept because three assumptions made before they existed turned
out to be false, and each is now asserted somewhere so it cannot come
back:

1. **Object ids do not carry their type.** They look like `u39c`, `ueb`,
   `ub8`, `d` — a prefix and hexadecimal number. The *file name* carries
   the type (`Stories/Story_u39c.xml`). An index built by prefixing an id
   with a type resolves nothing at all.
2. **Most objects have no part of their own.** Styles, fonts, layers,
   swatch groups and every frame inside a spread are declared inline,
   where they are used. Only stories, spreads and master spreads get a
   file each. An unresolved id is therefore usually an inline object, not
   a missing part, and treating it as an error would reject every file
   here.
3. **Not every document has a story.** `images.idml` and
   `placeholders.idml` have no `Stories/` directory at all, because
   neither has a text flow. A reader that assumed one would report a
   missing part for a document that is complete.

## Keeping them

`crates/codec-idml/tests/real_fixtures.rs` includes them with
`include_bytes!`, following the convention in `crates/codec-affinity`, so
the paths resolve at compile time and the suite cannot be broken by the
working directory `cargo test` picks.

If a file is ever removed, delete its entry from the `FIXTURES` table in
that test and the row above with it; the test asserts against the table,
so a fixture the table no longer lists is simply not checked.

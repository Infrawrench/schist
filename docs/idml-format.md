# IDML

How `schist-codec-idml` reads and writes IDML, what it relies on, and —
importantly — which parts of that are **verified** and which are still
**assumed**. The two are not the same, and the file format does not care
which one you are looking at.

IDML has a published specification (the IDML File Format Specification, with
RNC schemas per part), so unlike INDD this is engineering against a document
rather than reverse engineering a binary. That does not make it
self-verifying: the spec says what an element means, and only a real file
says whether a given producer emits it the way the spec implies.

## Status summary

| Layer | State | Verified against |
| --- | --- | --- |
| `container` — the ZIP/OPC package | **Done** | The system `zip` and `unzip`, in both directions, and seven real InDesign exports |
| `designmap` — root part and part index | **Done** | The same seven real exports |
| `import` — objects to `LayoutDocument` | **Done for the verified subset** | The same seven real exports, end to end into the layout kernel |
| `export` — a `LayoutDocument` back to IDML | **Done for the verified subset** | A round trip, the real exports rewritten, and the system `unzip` |
| File ▸ Open / Save / Save As | **Implemented, feature-flagged** | Registry routing and filesystem tests |

The container and part index were built before any specimen existed and
both were **wrong** in ways only a real file could reveal. See
[What the specimens proved](#what-the-specimens-proved) — it is the
clearest argument in this project for testing against somebody else's
output rather than your own assumptions.

## The container

An IDML package is an OPC/UCF archive: a ZIP with two rules a plain ZIP
writer gets wrong.

1. **`mimetype` is the first entry, and is stored, not deflated.** This is
   not a convention. A conforming reader is expected to determine the
   package's media type by reading the first bytes of the file, before it
   has a central directory to consult. A deflated or relocated `mimetype`
   makes the package unreadable to such a reader.
2. Everything else may be deflated as normal.

`container::write` emits `mimetype` first and stored whatever order the
parts were inserted in, and `container::read` takes an entry's sizes from
the central directory rather than the local header, which is what makes it
tolerant of a writer that streams an entry and leaves a trailing data
descriptor.

### Why the container is hand-written

The obvious choice is the `zip` crate. It is not used because the version
available in the local registry cache is **yanked**, and this crate has to
build offline. The replacement is small — end-of-central-directory scan,
central directory walk, stored and deflated — and the test suite
compensates by using the system's own `zip` and `unzip` as the other side
of every comparison:

- `zip` writes a package (stored and deflated) that this crate must read;
- `unzip` verifies and extracts a package this crate writes;
- `unzip -Z1` and `unzip -v` confirm `mimetype` is first and stored;
- a package read from a system-written archive and written back is still
  readable, which is what a resave does.

Those tests skip, visibly, when the tools are absent. On a machine with
them they are the only thing giving this code confidence, and they are the
reason to believe the reader will open a file it has never seen.

Scope limits, all deliberate: stored and deflated only, no encryption, no
multi-disk. Zip64 is *read* (sizes are taken as 64-bit and the extra field
is parsed) but never written. Embedded images can make packages large;
container and embedded-payload limits are enforced before decoding.

## The parts

Navigation is OPC's, not by convention. `META-INF/container.xml` names the
root part through `<rootfile full-path="…"/>`, and that — read, not assumed
to be `designmap.xml` — is where the document starts. (The metadata part is
`META-INF/metadata.xml`, not a `Metadata/` directory.)

The root part then lists every object part:

```xml
<idPkg:Story       src="Stories/Story_u39c.xml" />
<idPkg:Spread      src="Spreads/Spread_ueb.xml" />
<idPkg:MasterSpread src="MasterSpreads/MasterSpread_ub8.xml" />
<idPkg:Graphic     src="Resources/Graphic.xml" />
```

Those `src` paths are the authoritative map. These directory and filename
patterns are conventions, not identity rules:

```
Spreads/Spread_*.xml        Stories/Story_*.xml
MasterSpreads/MasterSpread_*.xml
Resources/*.xml             XML/…
```

### Three kinds of reference

A document contains three sorts of pointer, and confusing them is the
easiest way to write a reader that looks right and resolves nothing:

1. **Part paths** — `src="Spreads/Spread_ueb.xml"`, from the root part.
2. **Object ids** — `Self="u39c"`, `ParentStory="u373"`, resolved
   through `Parts::file_for`.
3. **Resource names** — `ParagraphStyle/$ID/NormalParagraphStyle`,
   `Color/Black`, `Ink/$ID/Process Cyan`. These name something *inside* a
   resource part rather than a part of its own, and are resolved against
   the resource parts, not the object index. `Color/Black` means nothing
   without the colour it names.

The prefix table is matched **longest first**, so `MasterSpread` is not
read as `Spread` and a master reference is not sent to a page part.

## What the specimens proved

Written before the fixtures existed, three assumptions turned out to be
false. All three were wrong in the same direction — about what a
conforming document always contains — which is the most dangerous kind of
wrong, because a reader built on any of them works on most files and
fails on the rest.

### 1. An object id does not carry its type

The obvious assumption is that a part's file name is its object's id, so
`Self="Story_u39c"` names `Stories/Story_u39c.xml`. Real ids are `u39c`,
`ueb`, `ub8`, `d`. The packaging element supplies the type and the
referenced XML supplies its `Self` id:

```text
<idPkg:Story src="Stories/Story_u39c.xml" />   file name ─┐
<Story Self="u39c">                               id     ─┘ together, the part
```

An index built by prefixing the id with a type resolves **nothing**. Opening
now builds the index from the actual XML `Self` attributes. Resource roles
come from the packaging elements, not `Styles.xml`/`Graphic.xml` filename
suffixes. Tests rename every listed part, use XML-escaped paths and vary
namespace prefixes; rebinding a familiar prefix to another URI invents no
parts.

### 2. Most objects have no part of their own

Styles, fonts, layers, swatch groups and every frame inside a spread are
declared **inline**, in whichever part holds them. Only stories, spreads
and master spreads are promoted to a file each. In
`bounded-text.idml` there are 38 object ids that resolve to no part at
all, against about 30 that do.

So an unresolved id is usually an inline object, not a missing part. A
reader that treated every unresolved id as an error would reject every
file in the set. `Parts::file_for` returning `None` is the normal case,
and `unlisted` exists so a writer can report what it skipped rather than
lose it silently.

### 3. Not every document has a story

`images.idml` places images and `placeholders.idml` holds barcodes.
Neither has a text flow, so neither has a `Stories/` directory. Two of the
seven. A reader that assumed a story part would report a missing part for
a document that is complete — and the failure would look like corruption.

## The object encoding, as read from the specimens

Implemented in `import.rs`, and recorded here because the observations are
the part worth keeping.

**Geometry** is points, in two attributes. A page:

```xml
<Page Self="uf0" Name="1" GeometricBounds="0 0 600 800"
      ItemTransform="1 0 0 1 -400 -300" AppliedMaster="ub8">
  <MarginPreference ColumnCount="1" ColumnGutter="12"
                    Top="0" Bottom="0" Left="0" Right="0" />
```

- `GeometricBounds` is **top left bottom right**: the example is 800×600
  points. The earlier x/y reading swapped non-square pages; Table 100 of
  the public specification corrected it.
- `ItemTransform` is an affine **a b c d tx ty** — scale, skew, translate
  — and it carries the position, while the bounds carry the size.
- `AppliedMaster` points at a `MasterSpread` part.

Text, graphic and shape frames retain the complete item and enclosing-group
affine. Bounds describe the local composition box; the matrix places it on
the page. Text wraps before transformation. Shape strokes are outlined before
transformation, retaining nonuniform widths under scale and shear. Groups are
flattened with an explicit report entry; clipped frame groups still warn.
Synthetic native XML tests cover reflection, quarter turns, nonuniform scale,
shear and nested group matrices over repeated saves. Preview hit tests and
print sampling use the same coordinate conventions.

**A story** nests paragraph and character formatting ranges. `<Br/>` ends
a paragraph; literal LF inside `<Content>` is a soft line break:

```xml
<Story Self="u373">
  <StoryPreference FrameType="TextFrameType" StoryOrientation="Horizontal"
                   StoryDirection="LeftToRightDirection" />
  <ParagraphStyleRange AppliedParagraphStyle="ParagraphStyle/$ID/NormalParagraphStyle"
                       Justification="CenterAlign">
    <CharacterStyleRange AppliedCharacterStyle="CharacterStyle/$ID/[No character style]"
                        FontStyle="Bold" PointSize="18">
      <Properties><AppliedFont type="string">Raleway</AppliedFont></Properties>
      <Content>Text with limited</Content>
      <Br />
      <Content>line and character count</Content>
    </CharacterStyleRange>
  </ParagraphStyleRange>
</Story>
```

A formatting range can span several paragraphs. The sample above therefore
becomes two `Point::Paragraph`s. `StyleRange` offsets include the model's
one-byte paragraph separators, and every character range is clipped to its
paragraph. `ParagraphBreakType` (also read as the older `GoToNextX`) selects
column/frame/page destinations. The public specification's examples 48–50
make these distinctions explicit; earlier code and tests incorrectly treated
Br as a soft break. The writer now emits native paragraph delimiters and
uses character references for soft line feeds. Structural breaks retain their
kind through repeated saves. Odd/even page parity currently degrades to an
ordinary page break with a warning.

**A text frame** names its story and threads to the next one:

```xml
<TextFrame Self="u370" ParentStory="u373" PreviousTextFrame="n" NextTextFrame="n"
           Name="..." ItemTransform="1 0 0 1 -260 181.97">
  <TextFramePreference TextColumnCount="1" TextColumnFixedWidth="720" ...>
    <Properties><InsetSpacing type="list">0 0 0 0</InsetSpacing></Properties>
  </TextFramePreference>
```

`InsetSpacing` accepts a scalar or a four-item list in top, left, bottom,
right order. The writer uses native ListItem children; asymmetric synthetic
round trips verify the mapping.

**Inks and colours** are in `Resources/Graphic.xml`:

```xml
<Ink Self="Ink/$ID/Process Cyan" Name="$ID/Process Cyan" Angle="75"
     Frequency="70" TrapOrder="1" InkType="Normal" />
<Color Self="Color/Black" Model="Process" Space="CMYK" ColorValue="0 0 0 100" />
```

The paint reference names **Color**, not the press Ink settings. `Model`
distinguishes Process and Spot, and `Space` determines the components: CMYK
percentages, RGB 0–255, or Lab. CMYK builds survive as authored channels;
RGB previews never become their separation source. The earlier importer
read Ink entries instead and reduced paints to black. A native fixture test
now compares the page-level polygon paints with their referenced Color
resources. Opaque Self IDs are resolved separately from names containing
slashes or Unicode. Per-fill/per-stroke overprint and object opacity use
native attributes and TransparencySetting/BlendingSetting respectively.
Registration colors are currently converted to process with a warning.

**Items belong to the spread, not the page.** A `<Spread>` holds its
`<Page>` elements *and then* the items, as siblings rather than children.
Object geometry and transforms place items in spread coordinates. Import
chooses the containing page (or nearest page for pasteboard items), subtracts
its origin, and stores page-local coordinates. Export applies the page's
spread offset. Synthetic multi-page tests translate the whole spread and
assert unchanged page-local geometry. A public facing-page fixture is still
needed to corroborate the mapping beyond single-page exports.

An absent item transform is identity. The earlier claim that it inherited
the page transform was incorrect. The background in `text.idml` now occupies
the full trim, rather than a negative-offset rectangle.

And the nesting that carries a frame's size is
`PathGeometry > GeometryPathType > PathPointArray > PathPointType`, where
**one `GeometryPathType` is one subpath**. Reading each `PathPointType` as
its own subpath yields a document with one shape per point.

**Linked images** are `<Link>` elements inside the part that places them,
not parts the root lists, and say whether their pixels are in the package:

```xml
<Link Self="u17c" LinkResourceURI="file:C:/.../restricted%20source%20folder.png"
      StoredState="Embedded" LinkResourceFormat="$ID/Portable Network Graphics (PNG)" />
<Link Self="u10a" LinkResourceURI="file:C:/.../linked.png" StoredState="Normal" />
```

An `Image` is a child of a Rectangle/Polygon/Oval frame, with its own bounds,
transform and Link. Embedded bytes are base64 in `Image/Properties/Contents`,
verified with a PNG in `images.idml`; assuming a `Links/` payload was wrong.
Original bytes are preserved in `LayoutDocument::assets`. Normal links refer
to external files and may need relinking. URL escapes are decoded once.

The writer emits this native image hierarchy and fitting geometry. Optional
Schist fitting intent uses the public `Properties/Label/KeyValuePair`
extension. The reader restores it only when link identity and native image
geometry still agree, so edits in another application take precedence.

Cubic contours preserve Anchor, LeftDirection (incoming), RightDirection
(outgoing), and PathOpen. All subpaths are written, including the closing
cubic. IDML uses nonzero winding; an even-odd layout path currently produces
an export warning. Repeated-save tests check anchors, handles, contour count,
closure and curve bounds to 0.001 point, allowing f32 extrema rounding.

These mappings use the [public IDML specification](https://raw.githubusercontent.com/jorisros/IDMLlib/master/docs/idml-specification.pdf)
and [public PathPoint documentation](https://developer.adobe.com/indesign/dom/api/p/PathPoint/),
plus the Customer's Canvas XML fixtures. No Adobe headers were consulted.

### One bug this found in another crate

`Rect::union` in `schist-layout` treated a zero-size rectangle as "no
value yet" and returned the other operand, which is the right behaviour
for an accumulator and the wrong one for a point — and they are the same
type. `ShapePath::bounds` unions one zero-size rect per path point, so
every shape was being placed at its *last point* rather than where it was
drawn. Nothing in the layout crate's own tests caught it, because nothing
in that crate built a rectangle out of points.

It is fixed, with `a_union_of_points_is_the_box_around_them` to keep it
fixed. Worth recording because the bug was in a crate two layers from
where it showed up.

## Fixtures

Seven real InDesign exports, in `fixtures/idml/`, with their provenance
and licensing reasoning in [`fixtures/idml/README.md`](../fixtures/idml/README.md).
They are plain XML inside a ZIP from a published specification, obtained
from a third party that publishes them for this purpose — no Adobe binary
was read, which is the rule `AGENTS.md` sets.

## Evidence limits

The container is cross-checked with system ZIP tools. Object mapping is
checked against seven published IDML exports and synthetic model cases.
Schist roundtrips alone do not prove that InDesign accepts the generated
files or renders them identically. No InDesign application validation has
been performed here. IDML geometry is decimal XML, not a binary fixed-point
stream.

### What is still needed

Reading is done for the subset the specimens cover, and every file reports
what it could not read rather than dropping it quietly. What is *not* yet
covered by a real file:

- **Facing pages and a spread of two pages.** `multipage.idml` has several
  pages; nothing yet has confirmed a two-page spread, its gutter, or
  `AppliedAlternateLayout`.
- **A master page with items overridden on a page.** A master is present
  in every file, so the inheritance *shape* is known, but no fixture
  exercises an override.
- **A spot ink.** Native Color Model=Spot is covered by synthetic tests;
  no supplied vendor fixture corroborates spot output.
- **Overprint**, and **table and footnote composition**.
- **A non-ASCII script**, which several of these files nominally declare
  fonts for (Minion Pro, Kozuka Mincho) but none of which actually sets
  Japanese or Arabic text.

File open/save is wired. Remaining fidelity gaps must be resolved or clearly
reported before the feature is enabled:

- Outer frame affines and independent
  image rotation, reflection, shear and scale are retained and rendered. Inner
  Image/ItemTransform and GraphicBounds map to normalized frame coordinates;
  fitting precedes that map, and the frame clips the transformed image. Native
  point geometry survives repeated saves without private labels. Fitting labels
  are accepted only when all native image corners still agree. Invalid image
  geometry stays unpaintable and produces an import/preflight diagnostic.
  Curved and compound graphic frames keep their normalized cubic clipping paths.
  A shared antialiased mask clips preview and print alpha after the outer affine,
  without changing native CMYK channels. Native outline edits invalidate stale
  clipping metadata independently of fitting metadata. The published oval image
  in `placeholders.idml` and repeated saves without labels cover this path.
- Local supported paragraph/character formatting becomes reusable named styles
  based on the original style, with a conversion notice. Opaque resource IDs
  and duplicate names in style groups resolve through an explicit map. Paragraph
  font/paint defaults inherit per property; character overrides win. Advanced
  properties (including arbitrary font variants, tints and some decoration
  attributes) still need representation and validation. Tracking uses native
  thousandths of an em, converted per effective run size during composition.
  IDML combines bold/italic into FontStyle; export preserves the resolved face
  but cannot retain independent inheritance of those two properties.
- Unsupported story content and alternate-layout sections need further work.
  Thread ordering, scalar/four-sided frame insets, numbering sections, asymmetric
  document offsets, paints and opacity have synthetic native XML checks; external
  application rendering remains unverified.
- Schist page visibility survives saves in a `Schist.PageVisibility.v1` page
  label. Export explicitly warns that visibility is retained in Schist only;
  it does not invent a native hidden-page attribute. The visibility tests cover
  every hidden-page combination and verify that unrelated labels never hide
  native pages.
- Even-odd fills have no direct IDML winding-rule equivalent.

Named styles, inheritance, font resources, layer membership/properties and
embedded images are now read and written. The earlier statements that
Styles.xml and object mapping were unimplemented were stale.

INDD has one public paired specimen, recorded separately in
[indd-format.md](indd-format.md). It does not validate INDD object semantics.

## Writing

`export.rs` is the mirror of the reader, and the two are held against each
other by `tests/round_trip.rs` in both directions:

- **Out and back.** A `LayoutDocument` written as IDML and read again is
  the same document: the page and its four margins, the frame's position,
  size, column count, gutter and insets, the story's text with its `<Br/>`
  as a newline, its character ranges over the right bytes, the master page
  and the page that applies it, and the inks **with their kinds** — a spot
  ink read back as a process one is a print error, not a cosmetic one.
- **A real file, rewritten.** A document read from a genuine export, written
  out, and read again. Harder than the round trip, because it starts from
  a file this writer never saw.
- **The system `unzip`** accepts what we write, so the package is not one
  only our own reader can open.

Three things writing got wrong, all found by those tests:

1. **Text outside every character range was dropped.** A range is a
   character *style* over some bytes, and the bytes outside every range are
   ordinary text. Filling only the styled spans deleted the rest of every
   paragraph that had any styling on it, which is most of them. The runs
   now cover the paragraph completely, with the unstyled spans written too.
2. **Resource parts were named `Resources/Graphic_u123.xml`.** The id is a
   property of the objects inside a resource part, not of the part itself;
   real files call it `Resources/Graphic.xml`. The writer now matches, so
   its output is shaped like a file InDesign wrote rather than one only
   this writer can read.
3. **A run carrying IDML's `[No character style]` became a style range**,
   putting an entry named `[No character style]` into the document for
   every text frame. Fixed in the reader, which is where the mistake was:
   most files have one of these on most frames, so the range list was
   mostly noise.

### Native preferences and style resources

`Resources/Preferences.xml` uses **DocumentPreference** (singular). The
multipage fixture sets 9-point uniform bleed and an 18-point slug offset
from trim. Both bleed and slug now retain all four offsets measured from trim;
this fixture maps to uniform bleed=9 and slug=18. A slug edge inside bleed is
preserved, even though it does not enlarge the media. Facing-page inside/outside
offsets map to physical right/left on pages left of the spread spine; they map
to left/right otherwise, independently of LTR/RTL reading order. Native export
sets the uniform flags only when all edges agree. Differing per-page logical
offsets still warn on export because IDML document preferences are global; only
the affected edges expand to their largest extent.

Numbering is a **Section in designmap.xml**, not a DocumentPreference
attribute. PageStart references the section's starting Page Self; PageNumberStart,
IncludeSectionPrefix and SectionPrefix are attributes. PageNumberStyle is a
Properties child with native enum values Arabic, LowerRoman, UpperRoman,
LowerLetters and UpperLetters. Every section resolves through the native page
reference, independently of XML section order. ContinueNumbering preserves the
sequence across style changes; explicit restarts reset it. Prefix contents and
the include flag remain separate, and Name/Marker survive saves. Length is
recomputed from adjacent boundaries after page edits. Unresolved/duplicate
references and inconsistent lengths are diagnosed. Alternate-layout attributes
remain unsupported and produce a notice.

The layout model stores an optional Section on its starting Page. Boundaries
follow reordered pages, are removed with deleted pages, and participate in the
same single undo step as that operation. Newly inserted copies inherit the
surrounding numbering rather than duplicating a restart. Before an explicit
boundary, the document uses Arabic numbering from 1. A redundant default section
on the first page is normalized away. Tests cover all boundary combinations,
restarts/continuations, styles, hidden prefixes, opaque page IDs, hidden pages,
page moves/deletions, undo and repeated saves. Asymmetric offset tests cover
both reading directions and every binding-spine position in a four-page spread.
These are specification-derived synthetic XML checks, not external application
validation of populated multi-section/facing documents.

Styles are children of RootParagraphStyleGroup and RootCharacterStyleGroup,
not an invented Resources wrapper. Fonts likewise sit directly in idPkg:Fonts.
Native justification names are LeftAlign, CenterAlign, RightAlign,
LeftJustified and FullyJustified. Story export references named styles without
copying unresolved defaults over their inheritance. Tracking values follow
[the published type-size units](https://helpx.adobe.com/indesign/desktop/format-and-style-text/tabs-indents-and-spacing/adjust-tracking.html):
thousandths of an em, converted to points only for the text engine. Mixed-size
and repeated-save tests enforce this distinction.

These changes follow the [public IDML specification, Section and
DocumentPreference schemas](https://raw.githubusercontent.com/jorisros/IDMLlib/master/docs/idml-specification.pdf)
and the public XML fixtures. None was derived from Adobe headers or program
binaries.

## Wiring into the app

`CodecPlugin` cannot carry this codec. Its `import` and `export` are typed
on `schist_core::Document`, the raster document — the same wall `ToolPlugin`
hits, and the same reason Design Mode's tools are a separate path.

So there is a **parallel trait**, `LayoutCodecPlugin`, in
`crates/plugin-api`, with its own registry list and its own
`layout_codec_for` lookup. Widening `CodecPlugin` to accept either would
put a layout engine inside the image editor's data model and make every
caller match on which it got.

`schist-codec-idml::IdmlCodec` implements it, and is registered by
`plugins/codecs-common` behind the same `design-mode` flag as the editor: a
build that can open a layout document but has no way to show one would be
worse than not offering.

`Workspace::load_file` asks the question before it commits, because the
answer decides which half of the window the file lands in. The open path
runs on a background thread as the raster one does, and installs the
result into the design state and switches to Design Mode. What the reader
could not read goes into the status line, not only the log: a user who
opens a document and finds a frame missing deserves to be told rather than
left to notice.

### The failure this guards against

An IDML offered to the **raster** decoder does not error. It produces a
small grey image the size of a page. So a routing mistake shows up as "the
file opened but it is blank", which looks like a successful open and takes
an hour to find. `tests/open_path.rs` asserts the two lookups stay apart
in both directions, and that a real export is recognised and reads.

A layout document also needs a name, and the format has nowhere to keep
one that survives a resave, so `LayoutDocument` grew a `name` field. The
open path fills it from the file's name.

## Checks

```sh
make check-idml
```

`tests/container_interop.rs` checks system ZIP interoperability;
`import_real.rs` and `real_fixtures.rs` exercise public exports. Graphic and
curve tests additionally inspect native XML structure and deliberate external
geometry edits.


### Thread references and ruler guides

TextFrame PreviousTextFrame/NextTextFrame references are resolved after all
spreads have been read. They determine story flow independently of XML child
order or page order. Dangling/cyclic references produce a report entry.
`tests/threading.rs` exercises reverse page order, reversed object insertion
order and repeated saves across two through six spreads.

Page-local Guide elements carry Orientation (Horizontal/Vertical), Location,
FitToPage=true and Locked. The public IDML specification's Spreads/Master
Spreads chapter lists this encoding; `tests/guides.rs` covers its synthetic
round trip, including negative locations. No supplied vendor specimen has
Guide elements, so external coordinate agreement is not yet fixture-verified.

Drop-cap character and line counts are read/written as native `DropCapCharacters` and `DropCapLines`, including style inheritance and local overrides. The published [ParagraphStyle property reference](https://developer.adobe.com/indesign/uxp/dom/api/p/paragraph-style/) documents both counts. Schist's legacy implicit one-character count is made explicit on native export. Horizontal initials now render from their actual glyph outlines; vertical initials remain a composition gap.

Direction evidence: the published IDML specification
(Stories paragraph properties and StoryPreference properties) distinguishes
`ParagraphDirection` from `StoryDirection` and `StoryOrientation`. The
[official direction guide](https://helpx.adobe.com/indesign/desktop/language-and-proofing/arabic-and-hebrew/change-text-direction.html)
and [scripting guide](https://developer.adobe.com/indesign/uxp/resources/recipes/rtl/)
confirm that story direction controls column progression; it must not be used
as a substitute for the paragraph's bidi direction. The codec retains explicit
paragraph directions and story preferences separately. `StoryDirection` changes
frame-column progression without changing paragraph bidi. Native vertical
stories now compose with vertical axes in preview, editing and print.

The public [ParagraphDirectionOptions reference](https://developer.adobe.com/indesign/uxp/dom/api/p/paragraph-direction-options/)
has only LTR and RTL. Automatic paragraphs therefore export an explicit
resolved `ParagraphDirection` on each range. Standard `Properties/Label` entries
retain Auto editing intent: style labels require the expected native direction,
and story labels require matching range index, style, text, break and direction.
Changed native data takes precedence over stale metadata. Repeated-save tests
check style stability and changed text, styles and directions; removing every
private label still preserves the original visible bidi result. The published
[IDML specification](https://raw.githubusercontent.com/jorisros/IDMLlib/master/docs/idml-specification.pdf)
includes Labels in the Story and ParagraphStyle property schemas.
Paragraph-specific writing-mode interchange and vertical initials remain gaps.
No Adobe headers or executable code were consulted for this evidence.


### Parent sheets, overlays and overrides

The published [IDML specification](https://raw.githubusercontent.com/jorisros/IDMLlib/master/docs/idml-specification.pdf)
describes `PageCount`, `AppliedMaster`, `MasterPageTransform`, `ShowMasterItems`
and page `OverrideList` Self references. The reader now keeps master page geometry,
assigns each item to its sheet, and converts overlays into page-local coordinates.
Export emits real master `<Page>` elements instead of a zero-page master spread.
Document-page overrides suppress shared items only on their destination page;
master-page overrides suppress ancestors only in their own template chain.
Hierarchies retain per-sheet base references, overlays and overrides. Cycles and
unresolved override IDs produce localized import diagnostics; iterative traversal
avoids recursive stack growth.

Cycle detection keys both the parent and its sheet. A valid chain can revisit
another sheet of the same parent; treating the whole parent as one graph node
incorrectly reported a cycle and omitted base artwork. A repeated-save regression
covers that acyclic case as well as the existing true-cycle test.

`tests/parent_pages.rs` covers cover/right and facing left/right pages, translations,
rotations, reflection, shear, hidden parent artwork, nested parent chains and both
override scopes across repeated native saves. Kernel tests verify unchanged shared
geometry, destination-page baseline grids, and page permutations with reversible
placement remapping. These are synthetic specification-based checks, not external
application confirmation. The supplied vendor masters are still single-sheet and
empty; no real specimen yet corroborates a populated facing master or override.
Cross-gutter artwork now has synthetic geometry, stacking and output checks;
external facing-page validation remains needed. Parent text threads now resolve master-frame NextTextFrame references alongside
ordinary-frame references. Composition keeps separate ordinary and parent flow
scopes, respects explicit frame order, and carries page breaks across template
sheets. Repeated native saves check uninterrupted UTF-8 text consumption and both
NextTextFrame/PreviousTextFrame chains. External populated-master specimens remain
needed to corroborate this behavior.

Spread binding and page-reading direction now survive import and export. The
[published specification, §10.3.6](https://raw.githubusercontent.com/jorisros/IDMLlib/master/docs/idml-specification.pdf)
defines page XML order relative to `DocumentPreference/PageBinding`; the
[Adobe IDML Cookbook, p. 14](https://community.adobe.com/havfw69955/attachments/havfw69955/indesign/632677/1/IDML_cookbook_9627253.pdf)
describes `Spread/BindingLocation`. The model stores physical slots from left to
right and the spine independently of the applied parent. Native pages are emitted
in reading order, with coordinates measured from that spine. Export disables
automatic page shuffling to retain these fixed spread slots.

Page moves retain direct artwork, parent assignment, overlays and override IDs;
the facing template is reselected for the destination slot. Removing a page moves
the spine index only when a left-side slot disappears. The entire edit remains
one undo step. `tests/binding.rs` covers every spine position in one- through
five-page spreads, variable widths and gutters, LTR/RTL page order, page moves,
removals and repeated native saves. These are synthetic checks of the interpreted
specification rules. A real RTL facing/foldout export is still needed to validate
the interaction between native binding indices and reading direction externally.

Native paragraph styles with omitted direction default to LTR. Inheritance keeps
that default, including Hebrew or Arabic text. Schist's distinct unset Auto intent
uses a validated `Schist.ParagraphDirection.v1` label with `AutoDefault`; a native
explicit direction always wins over stale metadata. `tests/automatic_direction.rs`
checks omitted root direction, child inheritance and subsequent native edits.


### Spread item order and crossovers

Spread items are emitted in their original stacking order, with each owner's
page origin applied to its geometry. The earlier page-by-page writer silently
reordered items that overlapped across a gutter. Repeated native saves now verify
global geometry and stacking for interleaved ownership/layers, two through four
pages, and both reading directions. An importer may assign a crossing item to a
different nearest page; its visible spread geometry and object order must remain
unchanged.

Canvas, single-page preview, separation and preflight now include same-spread
crossovers. This follows the published [pages and spreads model](https://helpx.adobe.com/indesign/using/pages-spreads.html)
and the IDML spread-sibling structure described above. Tests and the independent
PDF proof cover ordinary artwork and applied parent instances; an unapplied
neighboring master sheet is not implicitly instantiated. A real populated facing
master and external application validation are still needed to establish that
last behavior's native agreement.

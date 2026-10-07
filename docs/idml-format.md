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
flattened with an explicit report entry; clipped frame groups still warn. A
group's own settings (its text wrap, export options and transparency elements)
are not page items: a group wrap that is on is reported as not applied, and the
rest is not reported.
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
kind through repeated saves, including odd/even numbered page destinations.

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

Reading reports unsupported content. The public Penn State templates now cover
populated two-sheet parents, facing spreads and a native spot ink; their supported
page geometry, parent artwork and text survive repeated saves. OAC's independently
published v19.5 templates add Japanese prose and populated facing masters. The
reference PDFs were inspected, but no matching native application comparison has
been performed. Missing evidence includes real overridden parent items,
alternate-layout sections, RTL/foldout spreads and overprint. Whole and split text-only
footnotes compose in the supported horizontal column policies described below;
layout-dependent numbering and unsupported note structures remain explicit gaps. Table,
math and anchored-content composition remain unsupported even though the
academic template contains examples.

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
  properties remain partial; supported patterned/path decorations and their limits
  are documented below. Tracking uses native
  thousandths of an em, converted per effective run size during composition.
  IDML combines bold/italic into FontStyle; export writes the resolved face
  and retains independent Schist inheritance in a guarded extension label.
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

INDD has eleven acquired public pairs, with three redistributable pairs in the repository, recorded separately in
[indd-format.md](indd-format.md). It does not validate INDD object semantics.

### Gradient swatches and fills

Gradient swatches in Graphic.xml are typed: Linear or Radial, and stops in
order, each a colour or tint swatch with its Location and Midpoint as fractions.
A swatch whose stops name an unknown colour, or whose type, locations or
midpoints are out of range, is reported and left out. An item's FillColor naming
one fills it with the gradient, run where its GradientFillStart (in the item's
own path coordinates), GradientFillLength (the line's length or the radius) and
GradientFillAngle (degrees counter-clockwise) say; saving writes the swatch,
its stop colours and the item's settings back.

An item stating no start begins at its path's left and bottom edges, as long as
its path is wide. InDesign's own exports write exactly that when a gradient is
applied in its user interface: the `multipage` fixture's rectangle, from
(198.763, -48.701) to (444.569, 197.105), has GradientFillStart
"198.763 197.105" and GradientFillLength 245.806. The public
[`gradients` PDF](https://github.com/paged-media/core/blob/ffb7c8713125dc77403ec0983099f74ac2558517/corpus/generated/gradients.pdf) (SHA-256
`7966be59…f4e9`) and its
[generator](https://github.com/paged-media/core/blob/ffb7c8713125dc77403ec0983099f74ac2558517/crates/paged-gen/src/samples/gradients.rs) state none on five
360 × 200 pt rectangles. InDesign shades each in unit space mapped by
`360 0 0 -360` onto the rectangle's bottom-left corner: page 1's black-to-paper
axis runs along the width, page 2's cyan, magenta and yellow stops sit at 0, 50
and 100 % of it, page 3's radial gradient is centred on that corner with the
width as radius, and page 5's four stops at 0, 33, 66 and 100 %. Every stop mixes
linearly (exponent 1) at the even midpoint the sample gives; an uneven midpoint
is applied as the exponent that mixes evenly there, a Schist reading.

Plates and the composite take each pixel's mix of its two stops' inks, knocked
out or overprinted as the item's fill is; the canvas draws the same mix. A
radial gradient's offset highlight (GradientFillHiliteLength) is reported and not
drawn.

### Gradient strokes and gradient text

An item's StrokeColor naming a gradient swatch strokes it with the gradient, run
where its GradientStrokeStart, GradientStrokeLength and GradientStrokeAngle say,
in the item's own path coordinates as a fill's are; stating no start it begins
at the path's left and bottom edges and runs its width. The stroke keeps a solid
stroke's band, centred, inside or outside, and each pixel takes the mix where it
falls, so a centred stroke continues its end stops beyond the path. A stroke's
offset highlight (GradientStrokeHiliteLength) is reported as a fill's is, and
saving writes the stroke back. No public sample strokes with a gradient: of the
58 InDesign-exported PDFs of the public paged-media corpus at
[ffb7c871](https://github.com/paged-media/core/tree/ffb7c8713125dc77403ec0983099f74ac2558517/corpus/generated),
only the [`gradients`
PDF](https://github.com/paged-media/core/blob/ffb7c8713125dc77403ec0983099f74ac2558517/corpus/generated/gradients.pdf)
(SHA-256 `7966be59…f4e9`) holds shadings, five, each filling one rectangle, and
its
[generator](https://github.com/paged-media/core/blob/ffb7c8713125dc77403ec0983099f74ac2558517/crates/paged-gen/src/samples/gradients.rs)
applies gradients only to those fills. Strokes therefore follow the
specification's Table 68, which describes the stroke's start, length and angle
in the words it uses for the fill's, read as the fill evidence reads them.

Text whose FillColor or StrokeColor names a gradient (in a character or
paragraph style or a local range) is filled or stroked with it in output and on
the canvas, and saved; an underline or strikethrough in the text's own colour
takes the gradient too. Text has no path of its own, so the gradient runs over
the frame the text is set in, from its left and bottom edges over its width,
each glyph showing the part it sits on; no public sample sets text in a
gradient, so this is a Schist reading consistent with fills. The specification's
Table 104 gives text a GradientFillStart, Length and Angle, and its Appendix C
defaults are a start of "0 0" with a length of -1, as every TextDefault and
default paragraph style in the repository's seven fixtures writes; Schist reads
that length as stating no vector. A stated length and angle are drawn; a stated
start is kept and saved but reported and not drawn, being in an InDesign frame's
coordinates Schist does not keep. The gradient's first stop stands in as the
text's ink for anything drawing solid colour.

### Stroke caps, joins and alignment

An item's EndCap (butt, round, projecting), EndJoin (mitre, round, bevel),
MiterLimit and StrokeAlignment (centre, inside, outside) are read, saved and
drawn in output and on the canvas, locally or from an object style's Stroke and
Corner Options category; a value the specification does not name is reported.
Corners mitre by default at a limit of 4, InDesign's defaults: until now shape
strokes joined round. The public [`strokes-fills` PDF](https://github.com/paged-media/core/blob/ffb7c8713125dc77403ec0983099f74ac2558517/corpus/generated/strokes-fills.pdf)
(SHA-256 `6930e9d6…d036`) and its
[generator](https://github.com/paged-media/core/blob/ffb7c8713125dc77403ec0983099f74ac2558517/crates/paged-gen/src/samples/strokes_fills.rs) show a 6 pt
stroke on a 200 × 100 pt rectangle: centred, it straddles the path; inside, the
PDF fills and strokes a 194 × 94 pt rectangle 3 pt in; outside, a 206 × 106 pt
outline with square corners 3 pt out; the cap and join pages set PDF line caps
and joins. So an inside or outside stroke moves the path half its weight for the
fill and the stroke alike: inside, the fill loses the inner half of a centred
stroke's band and the stroke is the inner side of a band twice as wide; outside,
the fill gains the outer half and the stroke is the outer side. Open paths stay
centred. Arrowheads are still reported.

### Item stroke types

An item's StrokeType names a stroke style its package declares in Graphic.xml,
locally or from an object style's Stroke and Corner Options category, and is
drawn in output and on the canvas and saved: custom DashedStrokeStyle,
DottedStrokeStyle and StripedStrokeStyle resources, the ones text decorations
use (dash and gap lengths in points with their EndCap and
StrokeCornerAdjustment, dot spacing centre to centre with dots as wide as the
stroke, stripes as start and end percentages of the weight), and the built-in
`$ID/Dashed`, which dashes with the item's own StrokeDashAndGap, EndCap and
StrokeCornerAdjustment as the specification's page item table describes them,
and is solid without dashes. GapColor, GapTint and OverprintGap fill the rest of
the band a solid stroke covers with their own ink, as decoration gaps do;
without a gap colour the gaps are clear. The path is unchanged: dashes start at
each contour's first point and run on round it, joining round corners with the
item's join, and an inside or outside stroke is laid along the path moved half
its weight. Fitted, each stretch between corners and ends has a dash at both
ends, as straight decorations are fitted, so two dashes meet and join at every
corner; a closed contour without corners fits whole cycles. Stripes run across
the stroke from the edge on its left as the path runs, the outside of a
clockwise frame, and mitre at corners. Saving writes the reference, the dash
attributes and the gap paint, and declares in Graphic.xml any built-in style an
item names.

A reference the package does not declare strokes solid. The public
[`strokes-fills`
PDF](https://github.com/paged-media/core/blob/ffb7c8713125dc77403ec0983099f74ac2558517/corpus/generated/strokes-fills.pdf)
(SHA-256 `6930e9d6…d036`) and its
[generator](https://github.com/paged-media/core/blob/ffb7c8713125dc77403ec0983099f74ac2558517/crates/paged-gen/src/samples/strokes_fills.rs)
show it: pages 11 to 14 give 200 × 100 pt rectangles 6 pt black strokes of type
`StrokeStyle/$ID/Dashed`, `$ID/Dotted`, `$ID/Canned Dotted` and `$ID/Japanese
Dots`, none declared in that package's Graphic.xml, and InDesign strokes each
exactly as page 10's `$ID/Solid`: `re 197.638 370.945 200 100`, `6 w`, `4 M`,
`S`, with no dash array. The sample's striped, wavy and gap-coloured dashed
styles are declared in Styles.xml with attributes the specification does not
define and have no reference pages, and no corpus PDF draws a dash array, so
dash phase, fitting, stripe sides, the gap band and patterned alignment are
readings of the specification, not measurements. Other built-in styles (Japanese
Dots, Canned Dotted, Canned Dashed 3x2 and 4x4, the Thick and Thin family,
Triple, the hashes, Wavy, White Diamond), whose look the specification only
names, are kept, saved with their declaration, drawn solid and reported, as are
arrowheads.

### Blend modes and drop shadows

An item's TransparencySetting BlendingSetting BlendMode and its
DropShadowSetting are read, drawn and saved with its opacity. The public
[`effects` PDF](https://github.com/paged-media/core/blob/ffb7c8713125dc77403ec0983099f74ac2558517/corpus/generated/effects.pdf) (SHA-256 `ac21a8ce…c0260`)
and its [generator](https://github.com/paged-media/core/blob/ffb7c8713125dc77403ec0983099f74ac2558517/crates/paged-gen/src/samples/effects.rs) write each
item's blend mode as PDF's own, so Schist blends as PDF does: plate by plate,
on each ink's complement, so Multiply keeps the inks beneath and adds its own,
Screen and Lighten clear toward paper, and Darken keeps the darker of each; the
composite blends the same way. Hue, Saturation, Color and Luminosity mix across
channels; they are drawn Normal and reported.

The same PDF draws a drop shadow as the effect colour, at the shadow's opacity
and blend mode, through a soft mask: the item's shape moved right and down by
XOffset and YOffset and blurred. Page 10's paper rectangle, 240 × 140 pt with a
6 pt offset and size, masks its shadow with a 200 ppi image whose edge passes
half-strength within 0.2 pt of the moved rectangle; across sizes of 6 and 24 pt
that edge is one curve scaled by the size, a Gaussian of deviation half the size
to within 1.2 %, its tails cut about 1.2 sizes out. Schist casts the shadow from
everything the item paints, paper included, blurs it with three box blurs of
that deviation (within 0.03 of the measured curve) and lays it beneath the item
in the effect colour (black for `n`), at the shadow's opacity and the item's.
KnockedOut, the default, hides the shadow wherever the item's shape covers,
which shows when the item is transparent. The specification's defaults apply to
attributes not stated: Multiply, 75 %, 7 pt right and down, 5 pt soft. On the
canvas a shape casts its shadow from its fill and stroke and a frame from its
fill, or its stroke without one; text and images alone cast none there, and
blend modes draw Normal. Spread and Noise are kept and reported. Other effects
(inner shadow, outer and inner glow, bevel and emboss, satin, feather,
directional and gradient feather), knockout and isolation groups, fill, stroke
and content transparency, and a group's own effects are reported when applied;
at InDesign's defaults, as its exports write them on every item, they say
nothing. One key is added to all 150 catalogs.

### Text frame auto-size

A text frame's AutoSizingType, AutoSizingReferencePoint, minimum height and
width (UseMinimumHeightForAutoSizing with MinimumHeightForAutoSizing, and the
width's) and UseNoLineBreaksForAutoSizing are read and saved, and the frame is
fitted to its text when it is opened and whenever an edit changes it, the fit
undoing with the edit that caused it. Its geometry is its own: what is drawn,
wrapped around and saved is the fitted frame. The public
[`text-autosize` PDF](https://github.com/paged-media/core/blob/ffb7c8713125dc77403ec0983099f74ac2558517/corpus/generated/text-autosize.pdf) (SHA-256
`89becd60…46f97`) and its
[generator](https://github.com/paged-media/core/blob/ffb7c8713125dc77403ec0983099f74ac2558517/crates/paged-gen/src/samples/text_autosize.rs) grow a 240 × 40
pt HeightOnly frame anchored at its TopLeftPoint, holding twelve one-line 11 pt
paragraphs under a centred 1 pt stroke, to 156.856 pt: its bottom 0.5 pt below
its last baseline, the stroke's reach, its text 0.5 pt in, and a neighbouring
frame's text wrapping around the grown box. So a height fit is the frame's last
baseline and what lies below it, its InsetSpacing and its stroke's reach, at
least its minimum height, about its reference point (the top, centre or bottom
of the frame stays). Width-only frames take their longest line's width, their
text broken only where it breaks itself, and height-and-width frames that width
and the height of their lines: the specification's reading, as no sample sets
them. A frame stating no reference point keeps its centre, as every frame
InDesign's own exports in the repository's fixtures states. Threaded frames keep
their size: the public PSU literary template's InDesign export threads
HeightAndWidth frames 319.44 and 317.28 pt wide, which no fit explains.
Proportional fits, and auto-size on a multi-column, shaped or text-on-path frame,
are kept, saved and reported; one key is added to all 150 catalogs. An empty
frame keeps its size. Object styles' auto-size category is still reported.

### A frame's stroke and corner options

A text frame's stroke moves its text: the text area shrinks on every side by
half a centred stroke's weight, an inside stroke's whole weight, and nothing for
an outside stroke or a stroke with no colour, on top of InsetSpacing; the first
baseline moves down as much. The public
[`stroke-inset` PDF](https://github.com/paged-media/core/blob/ffb7c8713125dc77403ec0983099f74ac2558517/corpus/generated/stroke-inset.pdf) (SHA-256
`76b72a7b…b242`) and its
[generator](https://github.com/paged-media/core/blob/ffb7c8713125dc77403ec0983099f74ac2558517/crates/paged-gen/src/samples/stroke_inset.rs) measure this on
200 × 66 pt frames of 10/12 text: 0.25, 1, 6 and 12 pt strokes centred, inside and
outside, strokes with no colour, 4 pt insets, and 2/8/6/10 pt insets under a 1 pt
centred stroke (text 8.5 pt from the left, 10.5 pt from the right, 2.5 pt down).
Its 60.3 pt frames hold a fifth line unstroked and under a 0.25 pt centred stroke
but not under a 1 pt centred or a 0.25 pt inside one. InDesign fits that line by
its baseline, its descent hanging below the frame, and so does Schist's body text
in horizontal frames, columns and wrapped bands; it used to need room for the
descent, so a frame InDesign filled exactly overset its last line. A footnote
whose reference line no longer fits above the notes moves on by that line's
baseline in the same way. Footnote bodies and vertical text still fit whole line
cells: no native sample shows how InDesign fits a note's last line or a vertical
frame's last column.

A shaped frame's outline is now offset inward exactly by its inset and stroke:
every line keeps that distance from the outline in every direction. The sample's
frame with a 30 pt chamfer at its top-left sets its first and third lines 30 and
6 pt in unstroked and 32 and 8 pt in with a 4 pt inset and a 6 pt centred stroke:
the offset edge √2 times the distance further in, rounded down to a whole point.
Schist sets 32.9 and 8.9 pt. The outline used to be narrowed by the full distance
across a band that much taller, which set those lines 37 and 13 pt in.

Rectangles, text frames and image frames take corner options, locally or from an
object style's Stroke and Corner Options category: TopLeftCornerOption,
TopRightCornerOption, BottomRightCornerOption and BottomLeftCornerOption
(rounded, inverse rounded, bevel or inset) with each corner's radius, no more than
half the shorter side. The item keeps its rectangle and draws, clips and sets its
text in the cornered outline, so saving writes the plain rectangle and the corner
attributes and nothing is rounded twice. A rounded corner is the quarter-circle
cubic InDesign's PDF draws for the sample's 12 pt rounded frames, its handles
5.373 pt from the corner; there, as in Schist, the first line starts 12 pt in and
the middle lines reach the edge. Inverse rounded, bevel and inset corners follow
the specification's names; no native sample draws them. A corner without a
radius takes InDesign's 12 pt default, and a value the specification does not
name is reported and read square. The older uniform CornerOption and
CornerRadius alone are read square, as the generator notes InDesign leaves them.
FancyCorner is kept and saved but drawn square and reported, as are corners on a
polygon or other outline with corner points that is not an upright rectangle.
Ovals have no corner points, so their corner settings change nothing and are not
reported. Text wrapping around an image frame's clipping path also read that
normalized path as points, shrinking a contour wrap to a point at the frame's
corner; it now covers the frame.

### Readings from the public corpus samples

Importing every sample of the public paged-media corpus at that revision (59
packages built by its generator) found three things Schist read differently from
the specification: Color resources in the specification's `LAB` colour space
(Schist accepted only `Lab`, its own spelling, so Lab spot colours such as
the `swatches` sample's "Brand Ink" were reported unsupported and dropped),
Guides hanging off a Spread and naming their page by PageIndex (read as an
unsupported frame), and bullets of BulletCharacterType UnicodeWithFont (a
character remembered with its font, which is still that character; Schist now
sets it in the paragraph's font). Saving writes `LAB`. The other report entries
the corpus raises are values the specification does not define
(`ContourTextWrap`, `ProportionalOldStyle`, anchored `LineCapHeight` and
`AnchorLocation`), images without GraphicBounds, and text variables of types
InDesign itself leaves blank in the samples' PDFs.

### What the import report names

The report lists what Schist reads but does not set, once per kind. Settings
InDesign's exports carry at their defaults on every document change nothing and
are not listed: the layout name every section carries (only a second layout
name, or a section paginated from another layout's master, is an alternate
layout); default object styles' text-frame categories (one top-justified column
without insets, first baseline at the ascent, no auto-sizing, no column rules,
horizontal stories without optical margins) and effects categories whose
effects are all off; text-variable definitions no instance uses, kept as written
for an exact save. Retained story content is listed only when a structure stays
unset after reading: a table, anchored item, text-only footnote, known control
or variable instance with one valid definition is set. The PSU academic
template now reports only its Registration swatch, its lowered local formatting
and its flattened groups.

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

### Package-wide resource identities

The public [IDML specification](https://raw.githubusercontent.com/jorisros/IDMLlib/master/docs/idml-specification.pdf),
section 9.5.4 / printed page 33, requires each Self value to be unique across the
package and every reference to follow an identity change. Its spelling is not
prescribed. The exporter previously generated part IDs from u101 and page-item
IDs from 0x8000 plus their numeric model IDs. Those domains could overlap; the
addition could also overflow. Opaque imported language or numbering-list IDs could
collide with generated styles, colors, pages, fonts or the document itself.

Page items now have a separate generated identity domain shared by ordinary and
path-frame references. After generating parts, the codec collects their actual
Self values. Conflicting opaque resource IDs are assigned deterministic unused
identities on a temporary document copy, and typed language/list references follow
the remap before parts are regenerated. Newly generated language resources also
reserve numbering-list IDs and authored unresolved references. Minted resource IDs
cannot intercept an existing name alias or unresolved language/list reference.
Source documents and opaque recovery XML are unchanged.

A standard per-resource Schist.ResourceIdentity.v1 label preserves the authored
identity, resource class and supported native definition. After reading all native resources
and styles, Schist restores an original ID only while that definition still agrees
and the original spelling would not collide with another resource or retarget an
externally added reference. Native class/definition/reference edits stay authoritative.
Removing the label leaves a valid native graph; malformed or duplicate records
stay inert and stable through later saves. This preserves model identity without
requiring private metadata to resolve native references. It does not resolve IDs
inside unsupported table, variable or other recovery payloads as live content.

Ten properties compare IDs across all parts and verify referenced object types,
Unicode/escaped identities, separate resource kinds with equal source IDs, native
changes, missing/malformed/duplicate labels, unresolved aliases, source immutability
and repeated saves. Every checked-in public IDML fixture also saves three times
with unique native identities. Numeric-limit cases exercise page-item IDs without
arithmetic overflow. Existing mixed ordinary/path threads and creation-order guards
remain covered. Full local verification is recorded in Roadmap / Handoff.

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

Drop-cap counts retain native `DropCapCharacters` and `DropCapLines`, including
style inheritance and local overrides. The public schema bounds their integer
values to 0–150 and 0–25 respectively. Signed zero, leading signs/zeros and XML
whitespace are accepted; fractional, negative and out-of-range values are reported
without truncating or clamping them. Authored invalid counts are reported and
omitted from native attributes. Schist's legacy implicit one-character count is
made explicit on native export.

The legacy one-character default is materialized only where no valid native
ancestor already supplies it. Descendants keep their absent count, so changing
the parent after reopening still changes them. Dormant ancestors and explicit
zero/count overrides are not rewritten. Broken or cyclic chains retain the
explicit fallback rather than relying on another member to supply a default.
Properties vary hierarchy depth and storage order, save repeatedly, then edit
the parent; they also cover activation boundaries and malformed chains. This
avoids unnecessary overrides, but cannot preserve every legacy unset intent:
a first active child of an inactive native-zero ancestor still needs an explicit one.

`DropcapDetail` retains the full native signed 32-bit flag value, independently of
counts, through inheritance, explicit zero resets, local formatting and repeated
saves. Unknown bits and inactive values are not discarded. The public
[ParagraphStyle reference](https://developer.adobe.com/indesign/uxp/dom/api/p/paragraph-style/)
describes side-bearing, descender and Japanese grid flags; the
[drop-cap guide](https://helpx.adobe.com/ae_en/incopy/desktop/format-text/paragraph-formatting/drop-caps-nested-styles.html)
describes their purpose, including vertical initials, without specifying exact
numerical placement. Twenty observed public-corpus values are `1` with inactive
counts; they establish retention evidence, not active native geometry.

Horizontal and vertical initials use Schist's outline bounding-box reservation.
Vertical glyphs retain their upright or sideways placement; the opening outline
extends from the body's capital edge to the last reserved column center. The
body clears its inline ink and follows the paragraph's column progression.
Scale is calculated in paragraph-local coordinates so moving a frame cannot
change the initial's font size. This is Schist's geometry policy, not evidence
of native application agreement.
Active explicit native flags therefore produce an import/export diagnostic and a
Preflight error when composed. Dormant values do not. Native flag rendering
remains a gap. A used text path also reports active enlarged initials because
its single baseline cannot reserve additional rows; zero counts and one-line
formatting do not raise this error. Named initial styles now compose from a leading canonical
`AllNestedStyles` Dropcap record, as described below; no invented native
`DropCapStyle` attribute is emitted.

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
Paragraph-specific writing-mode interchange remains a gap; Schist vertical
initial composition is described above.
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
neighboring master sheet is not implicitly instantiated. The Penn State templates now corroborate populated facing masters; external
application validation is still needed to establish that behavior's native agreement.

### Direct ink tints

`FillTint` and `StrokeTint` are native percentages. The published
[page-item properties](https://developer.adobe.com/indesign/uxp/dom/api/p/page-item/)
and [text properties](https://developer.adobe.com/indesign/uxp/dom/api/t/text-default/)
define 0–100 as explicit tints and -1 as the inherited/overridden value. The
reader retains direct shape tints and optional paragraph/character style tints;
-1 remains unset in styles. Local text overrides use the existing reusable-style
lowering. Invalid, nonfinite and out-of-range values produce localized notices.
Export writes percentages without altering the base Color resource or opacity.

Property tests cover zero, fractional and full tints, process and spot identity,
independent fill/stroke values, style inheritance and local overrides through
repeated native saves. These are synthetic specification-based checks. Named
`Tint` resources are now represented separately, as described below.
Object-style paint inheritance and text stroke rendering are now implemented,
as described below. Their independent tints reach preview and separation.
External application validation remains needed.


### Solid text decorations

Native `Underline` and `StrikeThru` flags now reach composition, preview and print
through paragraph/character inheritance and local style overrides. The existing
native boolean encoding remains unchanged. A new repeated-save property test
checks inherited true/false values and local explicit false without style growth.
The renderer uses automatic font metrics for horizontal text and column-relative
lines for vertical text. Solid custom color, tint, weight, offset and overprint
properties are now represented independently for paragraph/character styles.
The public XML schema and Appendix B example specify `Underline*` and
`StrikeThrough*` property names, although the strike enable flag is `StrikeThru`.
Weight/offset `-9999` explicitly means Auto; absent values inherit. Color and line
type are children of `Properties`: `Text Color` is a string, swatches and
`StrokeStyle/$ID/Solid` are object references. The writer retains these native
encodings, named Tint references and explicit no-ink values. Local formatting
becomes stable named styles; repeated saves must not grow the style set.
The public [InDesign user reference](https://helpx.adobe.com/pdf/indesign_reference.pdf),
"Change underline or strikethrough options", confirms that offsets are baseline
relative: negative underline moves above, while negative strike moves below.

Invalid dimensions/booleans and unsupported line types are reported. Striped
lines, unadjusted dashes and gap paints are supported as described
below. Dotted and path decorations are described below; other built-in patterns remain unsupported.
No native-application agreement is claimed for automatic metrics or explicit
line placement; the PDF proof validates Schist's preview/output semantics.


### Explicit baseline offsets

The [published IDML specification](https://raw.githubusercontent.com/jorisros/IDMLlib/master/docs/idml-specification.pdf)
lists `BaselineShift` as an optional numeric attribute on paragraph/character
styles and story ranges. The [public CharacterStyle reference](https://developer.adobe.com/indesign/uxp/dom/api/c/character-style/)
distinguishes this unit value from the `Position` enumeration. Adobe's
[baseline guide](https://helpx.adobe.com/indesign/desktop/format-and-style-text/tabs-indents-and-spacing/adjust-text-baseline.html)
describes movement without changing leading. The codec now preserves explicit
point offsets, omitted inheritance and local zero resets through repeated saves;
supported local overrides reuse named styles rather than multiplying on save.
Malformed/nonfinite offsets produce an import diagnostic.

Native `Position` is independent of the explicit baseline offset. Normal,
superscript and subscript now import/export, including explicit Normal resets and
local overrides. Other native OpenType position variants remain diagnosed. Legacy
model script variants export as Position; explicit `BaselineShift::None` still
writes zero, while malformed numeric offsets produce a notice.

### Automatic superscript and subscript

Document `TextPreference` preserves SuperscriptSize/Position and
SubscriptSize/Position. The public XML schema accepts size 1–200 and position
-500–500; defaults in specification appendix C are 58.3% and 33.3%. The
[TextPreference reference](https://developer.adobe.com/indesign/uxp/dom/api/t/text-preference/)
and [character-formatting guide](https://helpx.adobe.com/ie/indesign/desktop/format-and-style-text/character-formatting/apply-drop-caps-text-positioning.html)
define size relative to nominal font size and displacement relative to regular
leading. Explicit point offsets remain additive. This implementation follows the
XML size range; zero is diagnosed rather than producing an invisible font.

Both public-domain Penn State v20.2 templates carry those defaults and a named
superscript footnote-number style; the academic template also has local superscript
ranges. Tests preserve those native positions and preferences through repeated
saves without generating more styles. This establishes attribute evidence, not
footnote layout support or matching native rendering. Tables, math, footnote and
anchored-content composition remain separate gaps.

The shared renderer retains nominal line metrics while scaling actual glyphs,
carets and selection segments. The output proof independently checks both script
positions in horizontal and vertical Japanese text, including explicit offsets,
regular paragraph spacing and equal-size super/subscript pixel displacement.


### Named tint swatches

Native `Tint` resources retain `Name`, `BaseColor` and a 0–100 `TintValue` as
specified by the public IDML schema (example 99) and [Tint reference](https://developer.adobe.com/indesign/uxp/dom/api/t/tint/).
The reader resolves base Colors before Tints, independent of part/element order;
missing bases, nested Tint references and invalid percentages are diagnosed.
Export includes the base Color even when only an inline named tint uses it.

A named tint owns its percentage. Native page/style paint references therefore
write `FillTint`/`StrokeTint=-1`; they do not multiply two percentages. The
[original Cell tint experiment](https://indiscripts.com/post/2021/05/cell-tint-enigma)
documents that assigning an explicit direct percentage detaches from the named
Tint to its base Color. Schist's direct controls follow that behavior. The model
retains named base identity so base-color edits update every use in one undo step.
Synthetic native XML tests cover opaque/forward references, percentages, invalid
resources and repeated saves. The current public templates do not contain named
Tint resources; acceptance by a native application remains unverified.


### OpenType features through composition

Named paragraph/character styles and local ranges read the native `Ligatures`
and supported `OTF*` boolean attributes. Figure styles expand to explicit
`tnum`/`pnum`/`lnum`/`onum` values; `OTFStylisticSets` is a 20-bit mask, including
an explicit zero reset. Missing tags inherit independently. These values now
reach per-range rustybuzz shaping rather than remaining unused style metadata.
The separate `KerningMethod` boolean also reaches composition.

Mappings use the public IDML specification's Stories/Styles attribute tables,
the [public CharacterStyle DOM](https://developer.adobe.com/indesign/uxp/dom/api/c/character-style/),
and Microsoft's [OpenType a–e](https://learn.microsoft.com/en-us/typography/opentype/spec/features_ae),
[f–j](https://learn.microsoft.com/en-us/typography/opentype/spec/features_fj),
[k–o](https://learn.microsoft.com/en-us/typography/opentype/spec/features_ko),
[p–t](https://learn.microsoft.com/en-us/typography/opentype/spec/features_pt) and
[u–z](https://learn.microsoft.com/en-us/typography/opentype/spec/features_uz)
registries. The named switch-to-tag mappings are inferred from those documented
meanings, not from proprietary headers. Public native fixtures corroborate
ligatures/contextual alternates and the disabled/default settings. Enabled
swash/alternate forms still lack native-application visual validation.

A native figure/set attribute replaces its entire group. A partial per-tag
override cannot faithfully express that inheritance in one native attribute.
Such overrides and arbitrary tags are retained in `Schist.OpenTypeFeatures.v1`
inside the standard Properties/Label extension, with a localized export notice.
They do not emit a misleading partial native mask. When a later external edit
adds a native feature attribute, its value takes precedence over overlapping
extension tags. Invalid attributes and labels are diagnosed. Boolean switches,
all figure styles, mask edges, inheritance, partial-group notices, UTF-8 local
ranges and repeated real-template saves have property coverage.

`OTFHVKana` and `OTFProportionalMetrics` choose mode-dependent features. Their
false states reset both relevant tags; activation selects the paragraph axis
as described below.
The shared engine preserves arbitrary valid four-byte feature overrides, but
Schist's named style controls currently expose on/off values only. Native
PSD/Affinity writing uses its existing fallback for per-run overrides, without
claiming editable native parity for them.


### Leading and automatic percentages

Native `Leading` is a Properties child with `type="unit"` for points or
`type="enumeration"` for `Auto`. Auto is an explicit inheritance reset, not a
missing value. `AutoLeading` is a paragraph attribute giving a percentage of
nominal type size (0–500); unresolved Auto uses 120%. Both survive named style
inheritance, local formatting lowered into reusable styles, and repeated saves.
Invalid values are diagnosed and omitted. Legacy Schist numeric leading JSON
remains readable; only Auto adds a string representation.

The public XML specification and the Penn State templates corroborate these
encodings. The public [leading guide](https://helpx.adobe.com/gr_en/indesign/desktop/format-and-style-text/character-formatting/adjust-line-spacing-with-leading.html)
explains baseline spacing and the largest requested value on each line; the
[ParagraphStyle DOM documentation](https://developer.adobe.com/indesign/uxp/dom/api/p/paragraph-style/)
defines AutoLeading as a percentage of type size. Font cell height is independent
of that spacing. Fixed leading no longer scales again for larger character runs,
and first lines/blank paragraphs have explicit geometry. The independent output
proof covers mixed sizes in horizontal and both vertical progressions; external
application rendering remains unverified.


### Exact font variants

Native `FontStyle` now retains the exact typographic subfamily, including Light,
Medium, Condensed and localized names. Font family and variant inherit independently;
a nearer explicit legacy bold/italic choice resets the inherited named face. Local
ranges use the same rule. `Resources/Fonts.xml` lists actual `Font` children with
`FontFamily`, `Name` and `FontStyleName`, including character faces that inherit a
paragraph's family. Delivery manifests retain `font_families` and add `font_styles`.
Missing faces are never renamed to the renderer's fallback.

The public specification's Stories example 51 uses Bold Condensed; Fonts section
6.1.1 and schema example 92 define the per-face resources. The engine uses the
[OpenType naming table](https://learn.microsoft.com/en-us/typography/opentype/spec/name):
typographic subfamily ID 17 takes precedence over legacy ID 2. This matters for
IBM Plex Sans Light, whose legacy subfamily is Regular. The unmodified OFL test
fixture and provenance live in `crates/text-engine/tests/fixtures/`. Unicode and
MacRoman names are decoded. Variable-font named instances and custom axes are not
implemented; unavailable exact faces remain preflight errors.

A `schist.font-choice` Label records original optional named/legacy fields alongside
the native name. It preserves independent bold/italic inheritance on Schist reload;
an external edit to native FontStyle wins over stale label data. Native consumers
see the resolved atomic face selection. Repeated-save properties cover opaque style
IDs, UTF-8 local ranges, common and nonstandard names, legacy inheritance, native
edits, resources and package manifests. All existing public-template round trips
also exercise the expanded model; external application rendering remains unverified.


### Object-style paint and frame appearance

ObjectStyle and AppliedObjectStyle now preserve the native paint subset:
BasedOn, EnableFill, EnableStroke, EnableStrokeAndCornerOptions, FillColor,
StrokeColor, StrokeWeight, FillTint, StrokeTint, OverprintFill and OverprintStroke.
Opaque style IDs and duplicate display names use the same reference table as
text styles. Missing bases/references and cycles are reported. Stored colours in
disabled categories do not apply; an explicit Swatch/None clears inherited ink.
Inline item attributes remain local overrides instead of flattening style paint.
RootObjectStyleGroup is written alongside the text style groups.

Evidence: the public IDML specification's Schema 161 and default object styles in
Appendix C, the public [ObjectStyle API documentation](https://developer.adobe.com/indesign/uxp/dom/api/o/object-style/),
and the published Customer's Canvas/OAC resource XML. NormalGraphicsFrame enables
fill/stroke while NormalTextFrame stores no paint. OAC's custom styles include
stored black fills with EnableFill=false, which must not turn text frames black.
BasedOn can be a native string or object reference. Tint -1 inherits.

TextFrame and image-frame inline fill/stroke now reach both preview and output.
Text frame paths are normalized to the frame; cubic outlines survive save/load and
shape the frame's text (see Text in shaped frames). Image frame
paint follows its existing clip_path. Unsupported enabled style categories,
arrowheads, built-in stroke styles Schist draws solid and decorative corners are
diagnosed; stroke options, stroke types and corner options are drawn (see Item
stroke types and A frame's stroke and corner options). No native application visual comparison
has been performed for this addition.

Unstyled legacy shapes retain their original paint fields. Styled shapes use
ObjectAppearance local overrides and ignore those legacy paint fields; import and
authoring neutralize them when attaching a style. This keeps raster documents and
ToolPlugin separate and lets old serialized layout snapshots read with defaults.

### Text stroke geometry and explicit no-ink overrides

Native paragraph, character and local text styles now retain StrokeWeight and
StrokeAlignment, independently of StrokeColor, StrokeTint and OverprintStroke.
The public [Character API](https://developer.adobe.com/indesign/uxp/dom/api/c/character/)
defines stroke weight as a measurement; it is stored as points, not a percentage
of fill or font size. The public [TextStrokeAlign API](https://developer.adobe.com/indesign/uxp/dom/api/t/text-stroke-align/)
distinguishes a centered contour stroke from one entirely outside the contour.
Both native enum values are supported. Appendix C of the published XML
specification contains an explicit OutsideAlignment paragraph style; it must
not be silently treated as centered.

Swatch/None and the native no-swatch marker are explicit no-ink overrides, distinct
from missing/inherited FillColor and StrokeColor. Named inheritance and lowered
local ranges preserve them through repeated saves. Existing layout JSON without
the new no-ink fields retains its former behavior. Nonfinite/negative weights or
miter limits, and unknown alignment/join values, are reported.

Native `EndJoin` retains MiterEndJoin, RoundEndJoin and BevelEndJoin, as defined by
the public [OutlineJoin API](https://developer.adobe.com/indesign/uxp/dom/api/o/outline-join/).
`MiterLimit` is an independently inherited ratio, with four as the default and
zero forcing bevels. Both paragraph and character properties, including lowered
local ranges, survive repeated saves. Older layout/engine JSON still defaults to
miter joins with limit four.

The renderer uses TrueType/OpenType font outlines and the shared vector stroke
rasterizer, with the selected join and miter limit. Fill precedes stroke; a centered
stroke straddles the contour, while outside coverage excludes its interior.
Equal inks union before opacity, and distinct inks retain separate coverage for
knockout/overprint and spot output. Native application visual comparison remains
outstanding; the PDF proof compares combined paints against independently placed
fill-only and stroke-only text objects.


### Mode-dependent CJK defaults

`OTFHVKana` and `OTFProportionalMetrics` retain absent, enabled and disabled
values independently on paragraph/character styles and local ranges. The public
[Character reference](https://developer.adobe.com/indesign/uxp/dom/api/c/character/)
and Microsoft feature registries above establish the horizontal/vertical
`hkna`/`vkna` and `palt`/`vpal` choices. Composition selects the paragraph axis.
Nearer switches reset inherited tags in their pair; nearer explicit tags win.

Independent per-tag exceptions still require the reported feature label.
`Schist.OpenTypeModeDefaults.v1` records the native baseline only for groups
that also contain private exceptions. External native additions, changes or
removals override stale exceptions without disturbing another group's values.
Malformed native switches and labels are diagnosed. Repeated-save properties
cover raw inheritance, false resets and stable lowering of local formatting.

The licensed bundled Noto CJK font verifies real proportional metrics in all
three writing modes through shaping, plates and a nine-page PDF proof. Its
feature tables contain no `hkna`/`vkna`; those switches have axis/precedence tests,
not a real alternate-glyph or native-application rendering comparison.


### Capitalization and small-cap preferences

The public XML attribute table/schema and
[Capitalization reference](https://developer.adobe.com/indesign/uxp/dom/api/c/capitalization/)
define `Normal`, `AllCaps`, `SmallCaps` and `CapToSmallCap`. These map to complete
capitalization flag pairs; absent attributes retain inheritance. Native local
formatting lowers into reusable styles without altering source text. Customer's
Canvas themes, bounded-text and shapes fixtures contain AllCaps ranges, now
covered through repeated saves. Their TextPreference parts corroborate SmallCap's
default 70 percent; the public specification gives its 1–200 range. Invalid values
are diagnosed, with safe defaults for invalid exported percentages.

The native capitalization property is atomic. A legacy style defining only one
of Schist's two flags uses `Schist.CapitalizationFlags.v1` and an export notice,
without pretending another application can reproduce its partial inheritance.
Native attributes added by another application override the extension. Unknown
native capitalization values are reported rather than silently called Normal.

The public [case-formatting guide](https://helpx.adobe.com/indesign/desktop/format-and-style-text/character-formatting/change-text-case.html)
describes small caps using native glyphs where available and scaled capitals
otherwise. Microsoft documents `smcp` for lowercase and `c2sc` for capitals.
The shared renderer probes each lowercase grapheme for an actual substitution,
retains marks and source clusters when synthesizing, and preserves nominal cells.
Licensed Noto and IBM Plex fixtures cover native and synthetic behavior. The
24-page proof matches independently authored uppercase, feature-tag and scaled
controls in every writing mode. Language-tailored case mapping is covered below;
native GUI/application rendering is not established by these tests.


### Striped decoration resources and gap inks

The public IDML specification's Graphics chapter (tables 129 and 132) defines
`StrokeStyle` and `StripedStrokeStyle`; the [public striped-stroke reference](https://developer.adobe.com/indesign/uxp/dom/api/s/striped-stroke-style/)
confirms percentage start/end pairs. The codec retains named stripe definitions,
including unused resources, resolves opaque IDs, and emits distinct IDs for
same-named definitions. Arrays must be finite, strictly increasing pairs within
0–100. Invalid resources and references receive diagnostics; invalid model values
are not emitted as fake Solid references. Duplicate definitions are deduplicated.

`UnderlineType`/`StrikeThroughType` independently inherit. A weight or color edit
does not reset an inherited pattern; explicit Solid does. `*GapColor`, `*GapTint`
and `*GapOverprint` retain separate paints, direct percentages, named Tints and
explicit no-ink. Text Color follows glyph paint; no gap value means transparency.
A nearer percentage detaches a named Tint to its base Color, matching other paints.
Local formatting lowers once and remains stable through repeated saves.

These are synthetic public-specification checks. Dash/dot corner adjustment,
other built-in styles and native application visual agreement remain unverified
or unsupported; no equivalence is implied for them. Path decoration rendering
is described below.


### Dashed decoration resources

The public Graphics schema/table 130 and [DashedStrokeStyle reference](https://developer.adobe.com/indesign/uxp/dom/api/d/dashed-stroke-style/)
define alternating point-valued dash/gap lengths, up to ten values. Schist retains
finite, nonnegative even arrays with a positive cycle, including zero dash/gap
members. They remain distinct from stripe percentages and scale with output DPI.
Named resources, unused definitions, opaque IDs, inherited properties and local
formatting survive repeated saves.

This subset writes native `ButtEndCap`, `RoundEndCap` and `ProjectingEndCap`,
with native corner-adjustment values described below. Cap choice contributes to resource identity;
same-named patterns with different caps remain distinct. Unsupported cap/fitting
values are diagnosed and their resource references are reported; they are not
imported as if their appearance were supported. Invalid arrays are
also diagnosed. Missing cap/adjustment attributes currently use butt/none defaults;
external application agreement for these defaults and decoration phase remains
unverified. The [public corner-adjustment reference](https://developer.adobe.com/indesign/uxp/dom/api/s/stroke-corner-adjustment/)
describes fitting but does not specify its exact numeric algorithm. No native
compatibility claim is made for unsupported fitting or built-in patterns.


Native cap definitions follow public Graphics table 130 and the
[InDesign cap reference](https://helpx.adobe.com/indesign/desktop/create-lines-and-shapes/edit-and-style-paths/line-stroke-options-and-settings.html):
round/projecting caps extend beyond dash endpoints. Geometric checks use the
[SVG 2 cap definitions](https://www.w3.org/TR/SVG2/painting.html#LineCaps) for circles
and half-width rectangular extensions, independently of the native codec. These
are public specification checks, not external InDesign rendering validation.

Public IDML example 75 uses DotArray values summing to
12 points. Figure 54 on printed/PDF page 279 shows the corresponding 12-point
pattern and a second dot center at 5.554 points. This establishes center-to-center
intervals despite the table's shorthand "gaps" wording. A local rendering of that
public specification page is retained as `/tmp/schist-idml-dot-style-reference.png`.
No custom stripe/dash/dot resources were found in the 17 acquired public IDML files
checked during this pass.


### Dotted decoration resources

Native `DottedStrokeStyle` resources retain one to five finite, nonnegative
center-to-center point intervals with a positive total cycle. The line weight
sets the circle diameter independently of those intervals. Zero intervals repeat
a center; overlapping circles contribute one silhouette. Named and unused
resources, same-name variants, opaque references, inheritance and local formatting
survive repeated saves. Invalid arrays and unsupported `StrokeCornerAdjustment`
values remain diagnosed rather than imported as a supported appearance.

The twelve-page dot proof matches independent analytic circles in all writing
modes. These are public-specification checks; external application agreement and
native fitting agreement remain unverified. No acquired public fixture has yet supplied
a custom dotted resource for an application-rendered comparison.


### Straight-decoration fitting

`StrokeCornerAdjustment` now retains None, Dashes, Gaps and DashesAndGaps for
custom dashed resources; dotted resources retain None, Gaps and DashesAndGaps.
Dash-only dot adjustment is still diagnosed because its behavior is not established
by the public reference. Adjustment contributes to resource identity, so same-name
patterns with different fitting stay distinct through opaque references, named
inventories, inheritance and repeated saves. Older serialized resources default
to None. Unsupported combinations are not exported as fake native resources.

The [public adjustment reference](https://developer.adobe.com/indesign/uxp/dom/api/s/stroke-corner-adjustment/)
identifies the adjustable components. Schist's renderer implements those rules for
straight decorations with a bounded proportional-fit search. Short fixed-dash
lines close gaps and clip the last dash; an adjustable zero first dash may grow
when no proportional solution exists. These numerical choices are Schist's and
have not been compared with InDesign output. Path corners and global fitting over
multiple path segments remain outside this subset.


### Language resources, shaping and case tailoring

The public specification, section 10.2.2/table 5, places `Language` declarations
in designmap.xml. Only Self and Name are required. The remaining attributes
include quotation pairs, primary/sublanguage names, Id and dictionary vendors;
standard Properties/Label metadata is also allowed. These are references to
installed dictionaries: adding a declaration does not install a new language.
The 17-file public corpus inventory is retained at
`/tmp/schist-idml-language-resources.json`. It contains both native resource IDs
and AppliedLanguage name aliases, notably English USA and Japanese.

The codec retains declared resources, including unused entries, and resolves Self
before Name. IDs are opaque even when they look like language tags. New authored
tags use an explicit tagged model value, bypassing that native namespace. Legacy
string values still load with their original identity-first semantics. Missing/empty
identities, duplicate IDs, invalid numeric Ids and unresolved AppliedLanguage
references are diagnosed. Paragraph and character defaults and local formatting
preserve their original references through repeated saves.

Authored tags normalize separators/case for shaping and pass RFC 5646 syntax checks,
without asserting registry membership. A finite mapping to names observed in the
public corpus supplies native references for supported tags. Arbitrary region or
script subtags are not dropped to force a match. Standard `Schist.Language.v1`
metadata preserves exact authored values, including explicit default resets, only
while the native reference and entire referenced declaration still match. Native
reference, dictionary or label changes invalidate that metadata. Unsupported native
mappings emit No Language and an explicit notice; Schist's tag stays in metadata.
This does not claim those tags render equivalently in other applications.

Language reaches the shared Rustybuzz shaper and its small-cap probes. The casing
implementation follows the Turkic and Lithuanian uppercase rules in
[Unicode 17 SpecialCasing](https://www.unicode.org/Public/17.0.0/ucd/SpecialCasing.txt),
using original source context and the
[After_Soft_Dotted definition](https://www.unicode.org/versions/Unicode17.0.0/core-spec/chapter-3/).
Soft_Dotted data come from Unicode 17 PropList; combining classes use the already
available unicode-normalization 0.1.25. Tag syntax follows
[RFC 5646](https://www.rfc-editor.org/rfc/rfc5646.html#section-2.1).
Romanian locl glyphs in the licensed IBM Plex fixture provide an independent
comparison with explicit comma-below Unicode text. Other CLDR case tailorings,
hyphenation/proofing dictionaries and native application agreement remain open.

## Native text paths

`fixtures/idml/text.idml` contains two `TextPath` children on open `Polygon`
parents (`u2f5/u309/u2f7` and `u398/u399/u39c`: parent/path/story). Both paths
have a 348.03286170167985-point straight baseline, start bracket zero, center
path alignment, baseline text alignment, Rainbow effect and zero path spacing.
The source attributes and checksum are recorded in
`/tmp/schist-text-path-evidence.json`. The public
[TextPath DOM reference](https://developer.adobe.com/indesign/uxp/dom/api/t/text-path/)
confirms point-distance brackets, terminal overset and mixed TextFrame/TextPath
thread references. The public [creation guide](https://helpx.adobe.com/sk/indesign/using/creating-type-path.html)
also limits a path to one line and excludes compound paths; paragraph spacing
does not add rows. The [effects guide](https://helpx.adobe.com/indesign/desktop/add-and-manage-text/type-on-a-path/apply-effects.html)
defines Rainbow placement at each character's baseline center and tangent.
These are public XML documents and API documentation; no
Adobe headers or executable implementation were used.

The reader now attaches those stories to Design path containers. A TextPath has
its own native ID: previous/next references target that child, while layers,
parent overrides, affine placement and frame paint retain the parent item ID.
The writer emits native Polygon/PathGeometry/TextPath structures, including all
cubic handles. Brackets retain point distances. An authored blank end follows
future geometry edits; a guarded `Schist.TextPath.FollowEnd.v1` Label preserves
that intent while the native EndBracket remains unchanged. Native bracket edits
supersede the label. Repeated saves retain the supported source geometry, stories,
thread order and bracket intent.

This subset supports one contour and one baseline per container, with horizontal
text following the baseline tangent (Rainbow), center-of-stroke path alignment
and baseline text alignment. Nondefault effects, flips, alignment, path spacing,
multiple content containers and invalid geometry/brackets are diagnosed.
When a native box or image also carries a TextPath, its primary content is kept
and the additional path container is reported instead of binding the wrong story.
Nonfinite imported brackets are diagnosed and sanitized before entering the
serializable model. Existing native underline/strike resources also render along
paths, retaining their independent line/gap paints and fitting settings. Schist
fits in baseline arc coordinates and bends the resolved masks; native corner
fitting and additional path effects remain open;
external application rendering has not been verified.

## Automatic lists

The public `multipage.idml` fixture declares 12 numbered paragraph ranges and
one bulleted range spanning multiple paragraphs. The first numbered range sets
`NumberingContinue="false"`; the remainder inherit continuation. Both examples
use `LeftIndent="18"` and `FirstLineIndent="-18"`. The document's default named
numbering resource does not continue across stories or documents. Exact XML and
the source checksum are retained in `/tmp/schist-idml-list-evidence.json`.
The published IDML specification defines the native list attributes, BulletChar,
TabList records, marker character-style references and NumberingList resources.
Adobe's public [list guide](https://helpx.adobe.com/indesign/desktop/format-and-style-text/lists-and-numbering/create-lists.html)
describes generated markers and their indent/tab controls; no executable or SDK
headers were used.

These properties now remain independent through style inheritance, local override
normalization and repeated saves. Opaque numbering-resource IDs, flags and labels
are retained. Unicode bullet values remain distinct from font glyph indices. NumberingFormat
retains its native string/enumeration distinction as one inherited value; existing
serialized string formats still load. The public schema permits both types.
Legacy numbered suffixes escape a literal caret before entering the native
expression grammar.
The compositor supports horizontal Unicode bullets and numbered lists at levels 1–9, explicit restart/continuation within each story, marker
alignment, marker character styles and left tab stops. Generated glyphs own no
story bytes. A legacy Schist literal-bullet gap stays in a guarded standard Label;
its native approximation is diagnosed, and later native list edits invalidate
the extension.

Specific/range restart policies, additional numbering formats, glyph-index bullets,
continuation across stories/books, non-left/leader tabs, and list combinations
with vertical text, enlarged initials or text paths remain diagnosed. Their
represented native settings are retained. Without explicit stops, Schist uses
the hanging indent when it clears the marker, then a 36-point tab interval;
import/export reports this policy because native implicit stops depend on ruler
settings. This is not an external-application rendering agreement claim.

### Counter-format integration

The [public NumberingStyle definition](https://developer.adobe.com/indesign/uxp/dom/api/n/numbering-style/)
and the IDML schema define Arabic, upper/lower Roman, upper/lower letters,
one/two/three leading-zero formats and FormatNone. These formats now compose;
conventional named strings and native enumerations retain their exact value/type
through inheritance and repeated saves. A repeated-save property exposed the
generic reader trimming literal format strings; this property now preserves
whitespace. Paragraph controls author enumerations
and preserve unchanged imported names. Native dictionary-specific formats remain
retained and diagnosed.

Letter counters use bijective base 26 (Z, AA, AB), independent of PDF page-label
repeated-letter numbering. Decimal padding is a minimum width of two, three or
four digits. FormatNone suppresses the number substitution while retaining the
expression's literal text and tab. Roman composition is limited to 1–3999; a
continued sequence that exceeds that range is diagnosed by preflight and both
codec directions without discarding the source settings. Higher-value native
Roman conventions and native application agreement remain unverified.

Synthetic properties cover every counter boundary through 20,000, very large
Arabic/alphabetic values, Roman decoding, named/enumerated inheritance and repeated
saves. The public fixture establishes the decimal named format; the other named
format tests are synthetic, not additional native specimens. The proof adds
independent literal Roman/alphabetic/padded strings, including digit transitions
and an empty marker. All 40 proof pages have been visually inspected without clipping or layout
changes between generated and reference pairs. The full counter-format sweep passes all eligible tests: 1,648 distinct Rust
tests plus four browser tests. The unchanged clipboard HTTP listener is the sole
sandbox-blocked test; native GUI/application parity remains unverified.


### Multilevel numbering integration

The [public multilevel-list documentation](https://helpx.adobe.com/indesign/desktop/format-and-style-text/lists-and-numbering/create-multi-level-lists.html)
describes per-level counters, higher-level references such as `^1`, restarting
after higher levels and disabling that restart. The published IDML example uses
`NumberingRestartPolicies` with `AnyPreviousLevel`, `LowerLevel="0"` and
`UpperLevel="0"`. These are public XML/DOM references, not SDK headers.

Levels 1–9 now compose in story order. Parent references use the referenced
level's number format. A new higher-level paragraph is a restart event even when
its number repeats the previous value. Explicit Start At still takes precedence;
automatic higher-level restarts use one. The implicit default sequence shares
the native default resource's identity, while equal display names never merge
distinct IDs. Counter results and measured marker plans are local to a composition
call and reused across columns and balancing trials. Resource inventories and
diagnostics batch their queries by story. No global document cache is introduced.

Specific-level/range restart-policy encoding is not established by the acquired
fixtures. Those policies remain retained and diagnosed when enabled. Missing or
stale parent references are also diagnosed; the renderer does not guess a zero
or reuse a parent from an earlier branch. Same-page cross-story support is described below.
The new hierarchy tests and proof cases are synthetic, and do not establish
native application rendering agreement.

The multilevel full sweep passes 1,654 distinct Rust tests plus four browser tests,
with the unchanged clipboard listener as the sole sandbox-blocked test. All 48
list-proof pages pass automated comparisons and visual inspection. A read-only
scan of the ten checked-in public IDML documents found only level-one counters
and the default restart policy; none establishes specific/range policy encoding.


### General paragraph tabs

The public IDML `TabList` record applies to ordinary paragraphs as well as list
markers. The [public tab guide](https://helpx.adobe.com/ca/indesign/desktop/format-and-style-text/tabs-indents-and-spacing/set-and-repeat-tabs.html)
describes frame-relative positions, explicit stops replacing preceding defaults,
and left/right/center/decimal alignment. Explicit source tabs now support all
four alignments. Literal leader painting is implemented as described below.
`Leader` and `AlignmentCharacter` are literal strings: import preserves their
whitespace. Missing, inherited, replaced and explicitly empty tab lists remain
distinct through repeated native saves. Source tabs retain their UTF-8 offsets.

The shared text engine accepts optional column-relative tab stops. Design passes
column-relative line starts during wrapping, including indents, generated markers
and enlarged initials, then uses the same origin for standalone line rendering
and carets. Implicit tabs use Schist's 36-point interval, which is disclosed rather
than presented as native geometry. Unsupported RTL, centered/right-aligned, path
and initial-tab combinations produce diagnostics. Preflight checks visible uses
with their original paragraph context, so a continued line beginning with a tab
is not mistaken for a tab inside its paragraph's enlarged initial.

Legacy raster text specifications default to no tab-stop model. PSD and Affinity
editable writers use their existing private/pixel or reported raster fallback
when this new setting is present. These properties and the new synthetic proof
verify Schist integration. No native reference was acquired at that checkpoint;
the later native collision observations are recorded below.

The paragraph-tab sweep and subsequent unrestricted publication rerun pass
1,666 distinct Rust tests plus four browser checks. All 408 editor tests now pass,
including the previously blocked, unchanged clipboard listener. All 24 new
tab-proof pages pass exact comparisons and visual inspection.


### Aligned source tabs

The public [TabStop properties](https://developer.adobe.com/indesign/uxp/dom/api/t/tab-stop/)
and [character alignment guide](https://helpx.adobe.com/ca/indesign/desktop/format-and-style-text/tabs-indents-and-spacing/specify-characters-for-decimal-tabs.html)
define right/center alignment and a single character for decimal alignment.
`RightAlign`, `CenterAlign` and `CharacterAlign` now anchor the following shaped
field at its end, midpoint or first matching character's grapheme caret. The
anchor uses actual styled glyph metrics, including vertical writing and caret
positions within ligatures. Source strings and UTF-8 offsets remain unchanged.
Missing alignment metadata in older TextSpec records still means leading stops.

The public guidance does not establish missing-character behavior. A character
absent from a field anchors its end; that fallback remains a Schist policy pending
native reference fixtures. The collision rule was corrected from a public native
PDF as described below. Empty fields still advance
to the next stop. Unknown alignment names, invalid character strings and leaders
remain intact and diagnosed. Generated list-marker spacing still diagnoses
non-leading stops; source-tab support does not implement marker alignment.

Justified paragraphs with non-leading tabs retain natural spacing and report
`Justification + TabList`: expanding the field could otherwise move its anchor
behind preceding text or jump to another stop. Existing RTL, paragraph
center/right alignment, vertical-path and initial-tab diagnostics remain.

The Paragraph panel has a collapsed Tabs section with one selected stop,
alignment icons, a position field and a character field only when relevant.
Editing an inherited stop creates a local list; deleting its last stop explicitly
clears that list. The inheritance action removes the override. Each action is
one history entry, and field commits reject stale captured records. Changing
positions or alignment preserves unrelated native leader/character metadata.

The independent print proof now has 48 cases (96 paired pages): four alignments,
three writing modes, two font sizes, first-line indents, process/spot inks, tint,
opacity, strokes and overprint. Reference frames are placed independently;
ordinary shaped field metrics determine the tab ruler anchors. It exposed
round-off just below integer glyph positions that moved entire masks by a pixel.
Glyph fill/stroke placement now snaps within f32 arithmetic precision before
flooring, keeping genuine fractional positions and all document geometry.

Pasteboard zoom also scales the tab ruler and its column-relative origin. A
preview property compares every tabbed caret with document-space composition
across four alignments, three writing modes, indents and five zoom levels.

The aligned-tab checkpoint passes all 16 roadmap verification targets and
1,687 distinct Rust tests. All 96 PDF pages match their paired reference samples
and page renders; contact-sheet inspection covers all 48 actual cases. Native
Schist Dev window checks cover alignment, position/character edits, add/remove,
inheritance, single-step undo and saving the selected native tab record. This
is Schist integration verification, not external InDesign rendering agreement.


### Source tab leaders

The public [TabStop leader property](https://developer.adobe.com/indesign/uxp/dom/api/t/tab-stop/)
and [tab guide](https://helpx.adobe.com/th_th/indesign/using/tabs-indents.html)
define a literal pattern of up to eight characters, formatted by the source tab.
Schist retains Unicode and whitespace exactly through native IDML saves. Inline
controls, line separators and patterns exceeding eight Unicode scalars remain
retained and diagnosed when used. Empty strings remove leader ink.

A selected explicit stop now paints complete independently shaped units, using
the source tab's resolved character style and paragraph direction. Unit caches
include direction; ligatures, synthetic capitals and font variants resolve with
the source face map. No source bytes, carets, original glyphs or wrap measures are
added or changed. Passed stops and implicit stops do not reuse a prior leader.
An ahead-of-pen stop whose field clamps to the pen has zero leader advance.
Schist fits whole units against the following field edge, with spare advance beside
the preceding text. This is a stated repetition policy, pending native reference
rendering. Nonpositive-width units generate no repeated ink. Enumeration happens
only during paint, with a finite glyph budget; rendering failure is reported in
both preflight separation paths.

The independent proof now compares 72 cases (144 paired PDF pages), including 24
new leaders across every stop alignment, writing mode and two sizes. Reference
leaders are ordinary repeated text in separate frames, with independently measured
periods and fixed field edges. Exact plates match at three resolutions, and all new
actual pages passed visual inspection. The complete leader sweep passes 1,696
distinct Rust tests, workspace clippy, app/native/browser/headless checks and i18n.
Native Schist Dev leader-field inspection remains pending because its running
window could not be located by the computer-use API. Native InDesign comparison
remains unverified.


### RTL source-tab rulers

The public [paragraph indent guide](https://helpx.adobe.com/indesign/desktop/format-and-style-text/tabs-indents-and-spacing/set-indents.html)
describes first-line right indents for RTL paragraphs, while the
[TabStopAlignment reference](https://developer.adobe.com/indesign/uxp/dom/api/t/tab-stop-alignment/)
names physical left/right/center edges and a character anchor. Schist maps those
physical names to the engine's logical ruler only after resolving paragraph bidi
from the complete source paragraph. Horizontal RTL rulers measure from the right
column edge; vertical inline progression remains downward. Applying the same
first-line indent from the leading edge is an inference from that public guidance;
no acquired native RTL tab fixture establishes rendering agreement yet.

Horizontal RTL source tabs now compose with right paragraph alignment. Vertical
RTL paragraphs retain a top-origin ruler with ordinary left/top alignment. Line
starts, first-line and hanging indents, wrapping, enlarged-initial reservations,
columns, threading, preview and paint share the resolved origin. Decimal anchors
use the shaped character's physical caret before converting to ruler distance.
Native stop names and paragraph/Story axes survive four repeated IDML saves.
Other paragraph alignment and justified non-leading combinations remain diagnosed.

A new print comparison found catastrophic cancellation while mirroring a final
field: two separate subtractions placed its glyph origin just below zero and
shifted its mask by a pixel. Mirroring now uses the stored field end. An exact
mask property covers seven sizes, five scales and four alignments. The expanded
proof compares 120 cases (240 paired pages), including 48 explicit/automatic RTL
cases with Hebrew and independently styled numeric fields, leaders, native stop
names, indents and all writing modes. Controls use ordinary tracked zero-width
padding rather than any tab implementation. Every control must fit on one line;
exact plates match at 72/144/216 dpi. All 48 new actual cases passed visual review.

Generated list-marker placement remains a separate gap: its left-origin spacing
does not implement RTL markers. These combinations are retained and diagnosed in
IDML and both preflight separation paths; they no longer paint a generated marker
at an incorrect column edge. Diagnostics resolve direction from the whole
paragraph, including continued lines beginning with RTL text.


### Native source-tab collision and edge observations

The public [tab-breaks PDF](https://github.com/paged-media/core/blob/2e3c998ea09101e028c626f5908a4db77a945b40/corpus/generated/tab-breaks.pdf),
[export metadata](https://github.com/paged-media/core/blob/2e3c998ea09101e028c626f5908a4db77a945b40/corpus/generated/tab-breaks.export.meta.json)
and [fixture inputs](https://github.com/paged-media/core/blob/2e3c998ea09101e028c626f5908a4db77a945b40/crates/paged-gen/src/samples/tab_breaks.rs)
provide an observable native reference: InDesign 20.0.1.32, horizontal LTR,
Inter 10/12, 200 pt frames with zero insets. Only fixture definitions and output
were consulted; no external importer, composer, renderer or Adobe header was read.
The PDF SHA-256 is `d99e6f4a27a07d2f0aadbb810f1700c26021a0429521914275338210334d6c46`.

Poppler word coordinates on page 2 show that ahead-of-pen right, center and
decimal stops clamp a colliding field to the pen. They do not skip to another
stop. Passed stops resume the implicit 36 pt grid. Page 1's beyond-frame stop
breaks after the preceding text; its following field starts on the next line.
The left-stop justified examples expand spaces only after the final tab. These
observations do not establish justified aligned tabs or other paragraph alignments.

Schist now selects stops by their ruler position before clamping the shaped
field. Zero-advance tabs keep their source bytes, carets and selected stop, with
no generated leader ink. The numeric test records all 52 sweep observations,
allowing 0.04 pt for glyph bearings and PDF advance rounding; it does not compare
font shaping or rasters. A wider property checks widths, ruler order and origins.
The same logical rule applies to other writing modes, without claiming native
RTL/vertical agreement.

A terminal tab after source text can end at the inline measure when its stop
is beyond it, allowing the following field to wrap. `TabStops.line_width`
carries that measure into standalone line paint, caret placement and zoom;
wrapping supplies its own per-line measure. Unbounded specs retain unbounded
geometry. A leading tab alone cannot create a blank line that consumes overset.
The property checks source coverage and standalone measurements in all three
axes, both explicit directions, several widths and first-line indents.

The independent print proof adds 18 touching-field cases across non-leading
alignments, axes and sizes, with spot/process paints, strokes and zero-gap
leaders. Controls place ordinary fields at the prefix's measured end. This
checks Schist's output integration separately from the native numeric evidence.

### Horizontal path/tab integration

Source tab stops use the start/end bracket's logical arc-distance ruler. Physical
LeftAlign/RightAlign names retain the horizontal direction mapping documented
above; indents do not move the ruler. Four native saves preserve cubic handles,
brackets, inherited stops, literal leaders, text, glyph masks and carets. Tight
cubic bounds are not translation-idempotent at f32 precision. A standard parent
Label (`Schist.TextPath.LocalBounds.v1`) retains authored local bounds only while
the native path geometry agrees; external geometry edits supersede it. External
item transforms still apply normally. No native curve semantics are invented by
this authoring-precision metadata.

The independently positioned ordinary-field/leader proof adds 64 path cases to
202 total cases (404 paired pages), with RTL, all stop alignments, two sizes,
first-line indents, three cardinal baselines and a cubic baseline. Exact plates
agree at 72/144/216 dpi. Glyph fill/stroke share a 1/64-pixel inline sampling grid;
carets and model geometry are unsnapped. This is a Schist raster sampling policy,
not a claim about native rasterization. Public native IDML establishes TextPath
structure, but native path/tab application agreement remains unverified.

### Initial tabs and generated marker leaders

A source tab included in DropCapCharacters remains an unsupported native setting.
Composition now falls back to ordinary source flow, preserving text, carets and
wrapping rather than enlarging the tab gap. The official [drop-cap guide](https://helpx.adobe.com/indesign/desktop/format-and-style-text/character-formatting/apply-drop-caps-text-positioning.html)
and [Paragraph DOM](https://developer.adobe.com/indesign/uxp/dom/api/p/paragraph/)
define counts and height but do not establish this combination's reservation or
ruler scaling. The retained native setting remains diagnosed; fallback is not a
claim of native drop-cap/tab agreement.

Horizontal LTR generated bullet/number tabs now paint literal leaders from the
selected explicit leading stop. Marker style, counter text, original marker masks
and source carets remain unchanged. A separate generated leader fragment uses the
column ruler; sharing a fractionally positioned marker frame shifted ink in the
independent proof. Passed/implicit stops and legacy fixed gaps cannot borrow a
leader. Four native saves retain valid literal strings, counter text and all
paint fragments. Non-leading marker tabs, RTL/vertical/path/initial marker
combinations remain retained and diagnosed. The 36-case list proof (72 paired
pages) includes 12 leader cases with independent ordinary-text placements. Native
leader phase agreement remains unverified.

### Native hanging-indent tab observations

The public [list-markers PDF](https://github.com/paged-media/core/blob/2e3c998ea09101e028c626f5908a4db77a945b40/corpus/generated/list-markers.pdf),
[export metadata](https://github.com/paged-media/core/blob/2e3c998ea09101e028c626f5908a4db77a945b40/corpus/generated/list-markers.export.meta.json)
and [fixture inputs](https://github.com/paged-media/core/blob/2e3c998ea09101e028c626f5908a4db77a945b40/crates/paged-gen/src/samples/list_markers.rs)
establish horizontal LTR placement in InDesign 20.0.1.32, Inter 10/12,
zero-inset 200 pt frames. Only the input definitions and published output were
consulted. No external importer, composer, renderer or Adobe header was read.
PDF SHA-256: `ed70b6d6232d2cfc8d2474dd1b507bb1589afe1bb5c6a019a7ca8119c7cd0141`.
Input SHA-256: `c0228d1e8272457c4c3689b2e01168b30d753aa1323eda369db3d474921fef44`.

Poppler word starts have a common 0.125 pt glyph bearing. Removing that bearing
gives these body starts relative to their frame:

| Cases | Input | Body start (pt) |
| --- | --- | --- |
| c00, c01, c08 | Hanging indent 18, tab after marker, no explicit stop | 18 |
| c03 | Bullet tab, no indent or explicit stop | 36 |
| c04 | Marker starts at left indent 18, no hanging indent | 36 |
| c05 | Left indent 50, first-line indent -20 | 50 |
| c06 | Left indent 50, first-line indent -50, explicit stop 30 | 30 |
| c07 | Left indent 30, first-line indent -30, explicit stop 60 | 30 |
| c10 | Number tab, left indent 50, first-line indent -50, explicit stop 10 | 10 |
| c11 | Number tab, hanging indent 6 lies behind marker | 36 |
| c12 | Ordinary `Tab\tc12 one`, hanging indent 40, no explicit stop | 40 |
| c13 | Bullet tab, hanging indent 18, passed explicit stop 2 | 18 |

An ahead-of-pen hanging indent is a virtual leading stop: an earlier explicit
stop wins, but a later one cannot suppress the indent. Schist previously placed
c07 at 60 and the ordinary c12 field at 36. Both now use the observed indent.
The numerical regression covers the 11 marker/tab cases; the ordinary-tab
regression checks c12. Neither claims font shaping or raster agreement.
Space-only marker cases c02/c09 are excluded from these numerical assertions.

The shared tab geometry keeps the virtual stop column-relative through wrapping,
standalone painting, carets and zoom. Properties cover origins, scales, ruler
order, passed stops and colliding explicit fields. A virtual stop has no leader;
an explicit stop at the same position retains its alignment and leader as Schist
policy, without native leader-phase evidence. Four repeated native saves retain
the indents, source bytes, masks and carets in both directions and all three axes.
Those additional axes are integration checks, not native placement validation.
The implicit-ruler notice now describes the hanging-indent/36-point fallback.

### Paragraph-local writing modes

Schist's paragraph styles can override the story's writing mode. Public IDML
`StoryOrientation` describes the story-wide axis; no native paragraph-local
counterpart has been established. Export previously omitted this model property,
changing saved vertical paragraphs to horizontal on reopening. Standard Label
metadata (`Schist.ParagraphWritingMode.v1`) now retains the explicit Horizontal,
VerticalRightToLeft and VerticalLeftToRight overrides. Missing or unrecognized
values do not introduce an override. Inheritance remains absent when unset.

Import and export report that paragraph orientation is retained in Schist only.
No native attribute is fabricated, and native application rendering of mixed axes
is not claimed. The hanging-indent property checks exact style overrides,
source text, masks and carets through four native saves in all three modes and
both directions. Native story orientation continues to use its ordinary field.

### Cross-story numbering: evidenced chronology and remaining limits

The public [numbering inputs](https://github.com/paged-media/core/blob/2e3c998ea09101e028c626f5908a4db77a945b40/crates/paged-gen/src/samples/numbering.rs)
declare one shared continuation resource for both pages, despite the second
page's restart label. The published [InDesign PDF](https://github.com/paged-media/core/blob/2e3c998ea09101e028c626f5908a4db77a945b40/corpus/generated/numbering.pdf)
shows 1/2 in story A and 1 in story B on both pages; its pages are pixel-identical.
PDF SHA-256: `5c96d7ceb03a1ec2e98a61b3bd54c1509fa62b591a4033356c26694e6b0be3a0`.
This discrepancy does not establish continuation or reset semantics. Fixture
comments about another renderer's expected order are not native evidence.

Adobe's public [list-options guide](https://helpx.adobe.com/indesign/desktop/format-and-style-text/lists-and-numbering/define-and-manage-list-options.html)
states that unthreaded frames on one page are numbered in frame-creation order.
Document story-vector order and paint order therefore cannot safely stand in for
that order. Schist now records object creation independently of stacking and
story storage. The creation transaction includes chronology in its single undo
step. Deleting an object keeps a chronology tombstone for undo; stories with no
ordinary frame do not contribute to the live list.

For one ordinary unthreaded frame per used story, all on one page and with known
creation order, a shared resource composes through those stories in frame order.
All source paragraphs participate, including overset. Ancestor restart events use
a monotonic paragraph ordinal across story boundaries, never a byte offset which
restarts at zero. Explicit restarts and independent resource identities remain
unchanged. The Paragraph panel's existing list disclosure now selects the sequence
and toggles continuation across stories. The resource setting is shared by its
referencing styles; changing it is one undo step.

Standard object Label metadata `Schist.ObjectCreationOrder.v1` retains Schist's
chronology through IDML saves, bound to the item's native Self and element kind.
An unlabelled import has unknown chronology; no order is inferred from the XML
walk or numeric IDs. Duplicate ordinals, native identity aliases, changed Self or
kind, malformed or repeated entries are rejected with a notice. Ordinals only
sort known entries and never control allocation. Deleted entries are omitted on
save, so relative order survives while ordinals may compress. External application
retention of this metadata is unverified, and identity guards do not prove that
an external edit retained original creation semantics.

Supported continuation reports a Schist-order warning on import/export and in
preflight. Unknown chronology, detached target stories, ambiguous resources,
multiple/threaded frames, parent instances, multiple pages and book continuation
remain explicit diagnostics without guessed markers. Properties cover storage
and paint permutations, multilevel events across offset resets, deletion/undo,
four native saves, identity invalidation, exact print plates at three resolutions
and both preflight paths. These tests establish Schist's behavior, not native
rendering equivalence. A controlled native continuation/reset fixture and broader
page/thread/template ordering evidence are still needed. No external importer,
composer or renderer implementation was consulted.


### Unsupported story structures: retention before composition

The public specification, section 6.4.1 and example 54, places inline tables,
footnotes and page items among CharacterStyleRange children, alongside Content.
The public Penn State academic fixture
`fixtures/indd/psu-academic-2/psu-academic-2.idml` supplies a Table in
`Stories/Story_u129a6.xml`, plus a Footnote and an inline Rectangle/MathObject in
`Stories/Story_u12666.xml`. Only its published XML was used for this change.
Previously those nodes generated an import warning and were then discarded.

The XML reader now retains the exact outer container bytes, including mixed
content, entities, whitespace and ACE processing instructions. Nested containers
stay inside that payload rather than becoming duplicate body text. Tables,
footnotes, TextFrame, Rectangle, Polygon, Oval, GraphicLine and Group containers
are stored as opaque story structures with UTF-8 byte anchors. A structure does
not insert source characters. Text edits retain anchors with right affinity for
insertions, refuse replacements crossing them, and reject invalid coordinates.
Story snapshots, styling, undo and redo retain the whole payload. Threading treats
opaque-only stories as occupied instead of orphaning their content.

The standard Story Label `Schist.StructuredStory.v1` retains the original Story
model whenever it contains these structures or legacy Other points. The writer
still emits only supported native body content. On read, it regenerates that
native representation and compares it to the actual story, ignoring XML attribute
order and insignificant whitespace. Native identity and resolved style names must
also agree. It does not trust a saved checksum or overwrite a native edit from a
stale Label. Changed body text, formatting, story identity or renamed resources
leave native text authoritative and retain the opaque data with unknown locations.
Malformed/duplicate Label values remain inert, unplaced recovery data. Subsequent
saves preserve that state without repeatedly nesting metadata.

Both import and export warn about unrendered retained content; unknown locations
have an additional notice. Both separation paths report missing structure paint
as an error even when ordinary body text fits. Story Editor shows its retained
count. All four warning/count keys, including the corrected existing omission
warning, are present in all 150 catalogs.

Tests hold the published table, footnote and math payloads byte-for-byte through
four saves; exercise every UTF-8 edit range, forced preservation through styling
and undo, legacy opaque points, external native changes and metadata corruption.
This establishes Schist data retention only. It does not reconstruct native style,
link or object resource graphs referenced inside opaque XML, compose tables or
footnotes, or establish native application/rendering parity. Those remain item 9
work; the feature flag stays disabled by default.


The next composition pass has public reference evidence: the `paged-media/core`
`footnotes` input fixture and its InDesign 20.0.1.32 PDF, pinned at
`2e3c998ea09101e028c626f5908a4db77a945b40`, were inspected without reading its
importer/composer/renderer. The one-page PDF visibly includes the first two notes
and only three lines of the third in a 120-point frame. Its reference digits are
on the body baseline and its note bodies have no printed number labels. The
fixture's stated 8-point default and requested rule settings must not be treated
as observed output. Native Penn State XML places FootnoteOption in
`Resources/Preferences.xml`; the input fixture requests one through designmap.
That placement difference needs testing before calibrating any numerical rule.
Source hashes, full-page render and word boxes are under
`/tmp/schist-native-footnote-reference/`; PDF SHA-256 is
`d3a62e560a4aa53e586e241f8003f8c8660b4f73aee9fc9ec3fc3690824ab89b`.

[Adobe's public footnote options guide](https://helpx.adobe.com/indesign/desktop/indexes-and-references/footnotes-and-endnotes/change-footnote-numbering-and-layout-options.html)
and [FootnoteOption DOM reference](https://developer.adobe.com/indesign/uxp/omv/f/FootnoteOption/)
separate numbering, marker styling, space reservation, rule geometry and splitting.
The [Footnote DOM reference](https://developer.adobe.com/indesign/uxp/dom/api/f/footnote/)
also distinguishes a note's anchor from its own text. Composition must
reserve space in the containing column and handle overflow explicitly; simply
painting a note at the frame bottom can overlap the body. These references are
requirements evidence, not a claim that structured composition is implemented.

### Document footnote preferences

Footnote options use typed layout data separate from each note’s own body.
The codec reads and writes `FootnoteOption` in the listed Preferences resource,
following public specification section 6.3.19, its enum tables and the Penn State
academic XML. The public [FootnoteOption reference](https://developer.adobe.com/indesign/uxp/omv/f/FootnoteOption/)
also documents the newer column-spanning option found as `EnableStraddling` in
that native specimen. No implementation code or proprietary headers were used.

Stored options cover start/numbering/restart policy, affixes and their placement,
paragraph and character styles, marker positioning, separator text, body/note
spacing, first-baseline settings, end-of-story placement, splitting and spanning.
Initial and continuing rules independently retain enabled state, geometry, stroke
resources, inks, gap inks, direct tints and overprint. Style references resolve
through the resources' actual Self values; they are not inferred from path-like
IDs. Resolved rule resources are exported even when used nowhere else.

Absence remains absence, without inventing a native default. Unsupported enum
strings and unresolved resource identities remain explicit and generate a notice.
Malformed numeric/boolean values are rejected individually with a property-specific
warning; they cannot overwrite neighboring valid settings. Prefix, suffix and
separator limits count Unicode characters. Native attribute whitespace uses
numeric references so tabs, CR and LF survive XML attribute normalization.
Settings transactions retain the options through one undo step and reject invalid
or unchanged drafts. Paragraph/character renames update the corresponding resolved
footnote style; swatch and base-colour edits update both rules and their gap inks
in the existing shared undo transaction. Unresolved identities are not guessed or
rewritten just because their strings resemble a renamed resource.

The public academic fixture retains its start at four, note style, tab separator,
7.2-point spacer, 0.72-point note spacing and 72/288-point rules through four Schist
saves, while its opaque note payload stays exact. Further properties exercise every
published numbering enum, all baseline/restart/affix/marker modes, independent
resources, opaque style IDs, later native changes, absent settings and invalid
ranges. These checks establish preference interchange, not native footnote layout.
Text-only body lowering and the supported reference-marker/space-reservation
path follow below; preflight continues to flag unrendered structures.


### Footnote body lowering

Public specification schema example 85 and IDML example 54 distinguish the note’s
text flow from its reference in the main story. The example and the Penn State
academic fixture use `<?ACE 4?>` for the note-body marker. The XML tree now retains
processing instructions at byte boundaries in decoded direct text, after entities
have been resolved. The marker is a typed coordinate in the note’s own story; it
adds no substitute character, changes no main-story byte offsets and does not
reinterpret literal digits as numbering.

Text-only footnotes lower paragraph/character ranges and local overrides through
the existing style reader. Reference styles in effect at the main-story anchor
are retained separately. The original outer XML remains exact. Notes with tables,
inline objects, nested notes, unknown instructions, mixed content or explicit
column/frame/page breaks keep only their opaque representation. This step does
not invent rendering for unsupported content.

Typed bodies travel through the existing snapshots and guarded Story Label.
Parent text edits retain note text and marker coordinates while moving the main
anchor; paragraph and character renames update the appropriate typed references
inside the same undo transaction. Native resource renames that affect only a note
also invalidate stale retention metadata, preserving its payload with unknown
placement instead of silently restoring the old style name.

Properties cover every UTF-8 marker boundary, entity decoding, paragraph offsets,
literal number text, local styles without repeated-save growth, native-only style
changes, opaque fallbacks, the public two-paragraph note and one-step undo across
multiple notes. Native text-only note export and whole-note composition are
described below. Splitting and full external application agreement remain open.


The matching public academic PDF (InDesign 20.2, Windows; CC0 at the same
[Zenodo record](https://zenodo.org/records/15800442)) was inspected on page 1.
It shows automatic marker 4, followed by literal 5 in the second paragraph of the
same note, below the right-hand text column and a separator. This corroborates
source structure; it does not establish full font/layout parity. PDF SHA-256 is
`b55beedb63512910077d37a46bfaa015f0c4801265701ad73aaa7bbdac43a58e`.
Observation metadata and extracted coordinates are in
`/tmp/schist-psu-footnote-reference.json`; no proprietary binary was read.


### Native text-only footnote export and hidden group artwork

Text-only typed bodies now write native `Footnote` containers inside their own
`CharacterStyleRange`, with `ParagraphStyleRange` bodies and zero-width
`<?ACE 4?>` instructions. This follows public schema example 85 / IDML example
54 and the public academic fixture; original raw XML is retained separately,
never copied into a package with stale resource references. Export splits source
runs at UTF-8 anchors, including empty paragraphs, terminal markers and multiple
notes at one position. Literal digits remain literal text. Unsupported bodies
and unknown or invalid anchor positions remain retention-only and diagnosed.

Automatic paragraph-direction guards exclude the independent text and breaks
inside structured containers; notes carry their own standard direction Labels.
Native notes remain readable after the Schist structure record is removed. Used
note-body font/face combinations join the shared native/package font inventory.
Repeated saves preserve exact original payloads without style growth. External
note text/style changes take precedence; old payloads remain recoverable with
unknown locations. A versioned guard flag recognizes earlier records that wrote
no native note body, without reviving a note deleted from a newer native export.
This establishes public-XML interchange, not native application or layout parity.
Schist numbering, reserved areas, continuation and painting are described below.

Native visual inspection of the academic fixture also found black corner squares
that are absent from its supplied PDF. The XML places those rectangles inside
visible groups on hidden Layer 3; magenta rectangles belong to hidden groups on
visible Layer 1. Flattening previously discarded the group layer and visibility.
Children now retain the nearest explicit layer and cumulative hidden state, along
with the already supported affine, locking and opacity. All 24 parent shapes
remain recoverable; none reaches page artwork while hidden. An object's hidden
flag is independent of opacity and layer visibility, survives snapshots and native
`Visible` attributes, and does not remove a hidden text frame from its story flow.
The compact Layers tree exposes an eye control with one-step undo. Nested-group,
ordinary/parent, repeated-save and legacy-serialization properties cover the rule.
Group editing semantics remain unsupported and explicitly reported.


### Whole text-only footnote composition

The initial composition path uses disposable projected stories for reference
numbers and ACE 4 body markers. No generated byte or style enters the saved
LayoutDocument or undo history. Every projected UTF-8 boundary maps back to its
source anchor; note bodies have their own read-only composed lines. Typed paint
runs retain spot identity, tint, opacity and overprint through PDF separation.

This path supports continuous Arabic, Roman, alphabetic, padded and full-width
numbers in horizontal single-column threads with explicit NoSplitting=true,
whole text-only notes, first-baseline leading/ascent and minimum offsets, solid
rules and end-of-story placement. It moves an unfit reference and note together;
an impossible fit stays overset. Independent multi-column areas are described
below, followed by explicit splitting support.
Page/spread/section restarts, additional numbering/baseline/rule policies,
vertical/path text and structured note content remain unsupported and reported.
Full native application placement agreement remains a validation task.

Primary public documentation:
- [Footnote options](https://developer.adobe.com/indesign/uxp/omv/f/FootnoteOption/)
  defines note spacing, first baselines, end placement, no-splitting and rules.
- [Footnote formatting](https://helpx.adobe.com/africa/indesign/desktop/indexes-and-references/footnotes-and-endnotes/change-footnote-numbering-and-layout-options.html)
  explicitly gives a marker character style precedence over the position option.
- [Detailed footnote layout help](https://helpx-origin.aws116.adobeitc.com/uk/indesign/using/footnotes.html)
  clarifies that paragraph spacing still applies inside multi-paragraph notes;
  note-area spacing replaces the outer first-before and last-after spacing.
- The [public reference manual](https://helpx.adobe.com/pdf/indesign_reference.pdf)
  shows zero default spacing, leading first-baseline positioning, and an enabled
  72pt solid rule at 1pt weight. These resolve absent values in the supported path.

The PDF proof compares generated references, note text and rules with ordinary
independently positioned text/shape frames, including spot paints, opacity and
rotation. It validates Schist's internal output contract, not external native
rendering agreement. The public academic specimen is also composed after native
saves to check that its generated 4 and literal 5 remain distinct and temporary.

Projected lines also retain their original paragraph context and numbering
outcome. Preflight must use those values before mapping note lines to the main
reference anchor: otherwise a tab or cross-story list inside a note could be
checked against an unrelated parent paragraph. Paragraph strings are shared
between their lines. Regression coverage exercises both separation paths,
valid local note lists, unsupported tabs and unsupported cross-story note lists.
List markers are measured from the authored main/note paragraph before inline
reference insertion, then their anchors are mapped before generated text at the
same boundary. References cannot replace the source font, paint or script
position used by a bullet/number. Explicit marker styles still override the
source context, and main-story counters retain document frame chronology.
An eight-page proof compares ordinary source-context markers with explicitly
styled markers through whole/split notes in every process/spot plate and Poppler.
This preserves Schist's source-context policy; it does not establish native
application agreement for these combined settings.
The independent rotated-rule proof caught a separate edge blur: separator vector
geometry now receives the frame affine before antialiasing, just like ordinary
shapes, instead of resampling an already rasterized line.

### Frame footnote policies and column areas

Public PSU academic/literary packages contain TextFrameFootnoteOptionsObject on
the Document and on ObjectStyle definitions. Their native properties are
EnableOverrides, SpanFootnotesAcross, MinimumSpacingOption and
SpaceBetweenFootnotes. Object styles gate this category with
EnableTextFrameFootnoteOptions. Disabled overrides still retain their spacing
and spanning choices; an explicit false is different from an inherited value.
The model preserves document creation defaults separately, copies them into new
rectangular frames, and retains local/style settings through snapshots, duplicate,
style detach and native saves. The [public span-footnote help](https://helpx.adobe.com/indesign/desktop/indexes-and-references/footnotes-and-endnotes/span-footnotes.html)
describes per-frame enable-overrides and spanning controls independently of the
document footnote policy. No proprietary header or executable was used.

With spanning explicitly disabled, whole notes use the column containing their
reference. Balancing trials shorten only the body region; a bottom-aligned note
still belongs at the actual column bottom. End-of-story placement follows the
body text when requested. LTR/RTL column order and forced column breaks share the
ordinary story flow. Layout-dependent restarts remain
an explicit unsupported policy; split-note continuation is described below. Pixel/plate reference proofs validate Schist's
internal placement and paints; full native application agreement is still pending.

With spanning explicitly enabled, a shared note area uses the frame's full inset
width and reserves space below every body column. A bounded height search keeps
complete notes with their references, including when adding the next reference
would require more footer space than the frame can provide. It shapes ordinary
paragraphs under the same widow, keep and grid rules; it never clips a source
prefix to manufacture a fit. Each trial owns its break cursor, so only the chosen
layout advances column/frame/page breaks. A note is measured once per frame
search. Balancing then minimizes the body height with the shared footer reserved.
Bottom and end-of-story placement use the same positioning and rule painter as
independent column notes. An absent spanning preference in a multi-column frame
remains unsupported rather than guessing a version-dependent native default.

The [public TextFramePreference DOM](https://developer.adobe.com/indesign/uxp/dom/api/t/text-frame-preference/)
documents frame spanning overrides; the public span-footnote help linked above
describes spanning all columns within a frame. This extends the existing whole,
horizontal, continuous-numbering, explicit no-splitting subset. The separate
continuation path below supports split notes. Spanning paragraphs, other structured
note bodies and native rendering parity remain outside this subset. Regression and independent text/shape proof results are recorded in the
roadmap handoff.

### Split text-only footnote continuation

NoSplitting=false uses an independent cursor for each projected note body.
An omitted NoSplitting has the same false default under Appendix C of the public
IDML specification. The model retains absence; composition resolves the default
without authoring an explicit value or changing native saves. Main-story EOF does not finish a thread while a referenced note has text
left. Pending notes can occupy later columns or note-only frames; clipping or the
last frame reports ordinary terminal overset even when all main bytes were placed.
Reference/marker projection and native source retention are unchanged.

Each area reserves legal note prefixes before distributing remaining room in
source order. It first reduces the body to the height required by its references;
whole notes remain whole when they fit there. Multiple references on one line can
start multiple continued notes. Paragraph keeps still constrain every fragment,
including whole paragraphs and adjacent bindings. If a new note cannot start,
its reference line moves forward. Trials copy both the main break cursor and all
note offsets, committing only the chosen result. Independent columns follow the
story's reading direction; spanning areas use the full inset width. Final-frame
balancing requires both main text and every note to fit.

A continued area's first rule comes from ContinuingRule properties, independently
of the first-note rule. The published
[IDML specification, Appendix C](https://raw.githubusercontent.com/jorisros/IDMLlib/master/docs/idml-specification.pdf)
defaults the continued rule to 288pt and the initial rule to 72pt; the public
IDML corpus also has ten explicit 288pt values.
Solid rules retain tint, spot/process paint and overprint. End-of-story placement
waits until pending notes finish; a note-only final area starts at the frame top
when that preference is enabled. No source, style definition or history is mutated.

The [public FootnoteOption DOM](https://developer.adobe.com/indesign/uxp/dom/api/f/footnote-option/)
describes flow to succeeding columns when the area reaches a reference and the
separate continued-rule properties. The reference manual linked above describes
paragraph keeps preventing individual notes from splitting. These public semantics
and independent text/shape controls guide this implementation; its numerical area
allocation policy has not been compared with native application rendering. The
public academic fixture uses no-splitting, while the nine acquired IDML packages
with NoSplitting=false have no actual footnote bodies. They establish preference
encoding, not continuation geometry. Explicit breaks inside notes, nested note
structures, vertical/path notes, unknown spanning defaults and layout-dependent
number restarts remain unsupported and diagnosed.

### Native column balancing

TextFramePreference.VerticalBalanceColumns is retained independently on frames,
object styles and document creation defaults. The public literary fixture has
an explicitly balanced two-column frame and false style/document defaults.
The [public TextFramePreference DOM](https://developer.adobe.com/indesign/uxp/dom/api/t/text-frame-preference/)
identifies this optional policy. Absent local values inherit enabled
EnableTextFrameGeneralOptions styles; false values remain explicit overrides.
The general-frame category remains partially supported, so its existing category
warning remains enabled. Balancing support does not imply support for all of its
other properties or native defaults.
Legacy Schist snapshots retain their balancing preference. Newly authored
frames copy the document's explicit creation default (initially false).


### Native paragraph keeps

ParagraphStyle and local ParagraphStyleRange properties retain KeepLinesTogether,
KeepAllLinesTogether, KeepFirstLines, KeepLastLines, numeric KeepWithNext and
KeepWithPrevious independently. The public corpus includes disabled keeps with
first/last counts of two and academic body styles using keep-with-previous.
Absence inherits; explicit false/zero overrides; disabling line keeps retains
its inactive counts and whole-paragraph choice.

The [public ParagraphStyle DOM](https://developer.adobe.com/indesign/uxp/dom/api/p/paragraph-style/)
and [public reference manual, Keep Options](https://helpx.adobe.com/pdf/cs6/indesign_reference.pdf)
describe these policies. The [public ChangeTextPreference DOM](https://developer.adobe.com/indesign/uxp/dom/api/c/change-text-preference/)
specifies first/last counts of 1–50 and next-line counts of 0–5.
The composer binds the preceding final line to the requested following lines,
or the entire following paragraph when shorter. A failed binding moves the
smallest complete suffix allowed by that paragraph's own widow/whole policy;
heading chains propagate backwards. Explicit column/frame/page breaks take
precedence. The pass also covers orientation changes and preserves opening
initials with their inset body lines. Full native application placement agreement,
including short following paragraphs, remains unverified. Native paragraph-start
constraints take precedence over bindings to adjacent paragraphs.

Older Schist keep_lines/keep_with_next fields remain readable and resolve into
the typed policy. A Schist.ParagraphKeeps.v1 label retains their exact authored
representation, only while every emitted native keep property still agrees.
Malformed or changed native values invalidate the label. Out-of-range authored
counts produce the existing localized invalid-preference warning, omit that
native count and retain its original value in the guarded label. Native import
reports invalid attributes rather than coercing them into valid settings.

Regression coverage checks every available split, independent inheritance and
explicit resets, disabled/whole/asymmetric policies, next-line counts and previous
bindings, chains in balanced LTR/RTL columns, mixed orientations, forced breaks,
opening initials, repeated native saves and stale-label invalidation.


### Native paragraph starts

ParagraphStyle and local ParagraphStyleRange StartParagraph retain Anywhere,
NextColumn, NextFrame, NextPage, NextOddPage and NextEvenPage. Absence inherits;
explicit Anywhere resets the inherited constraint. Unknown native values are
reported rather than coerced. Local overrides are interned without growing the
style table through repeated saves. Existing Schist snapshots deserialize the
new optional property as absent.

The [public StartParagraph DOM](https://developer.adobe.com/indesign/uxp/dom/api/s/start-paragraph/)
describes the six destinations. Composition treats them as zero-width constraints,
separate from unconditional StoryPoint breaks. A prior explicit break or natural
flow can already satisfy the destination. The initial paragraph accepts its first
container except when its numbered page has the wrong parity. Odd/even choices
use section numbering, including restarts, and skip every remaining frame on an
unsuitable page. This initial-container interpretation and precise placement have
not been checked against an external native application; the public corpus only
contains explicit Anywhere values.

Empty paragraphs still seek their required destination; unavailable destinations
leave source text overset. Satisfied constraints do not disable column balancing.
Logical column order follows the story direction. Path threads and mixed writing
regions use the same boundary state. Main-story constraints also retain whole and
continued footnotes, with trial cursors isolated during fitting. Note-body start
constraints remain retained and preflighted as unsupported structured content;
they are not silently treated as Anywhere. Explicit native Br odd/even variants are retained and composed as separate
forced-break points, as described below.

Paragraph exposes the choices in its collapsed Keep options group. Edits capture
the named style, preserve unrelated native/legacy keep settings and undo once.
The group inherit action clears both starts and keeps together. Seven short keys
are present in every existing locale catalog. Tests cover inheritance/local resets,
malformed native values, source-safe destinations, section parity, empty/overset
paragraphs, explicit breaks, balancing, path text, notes and exact undo/redo.

An independent output proof enumerates ordinary text-frame destinations for all
six policies, one/two columns, both story directions and numbering starting at
one/two. Process and spot plates match exactly at 72/144/216 dpi. The 24-page PDF
matches its controls in both Poppler image samples and rendered pages, and every
page has been visually inspected. This does not establish native application
agreement. The proof runs as part of `make check-design-output`.


### Explicit numbered page breaks

ParagraphBreakType and GoToNextX values NextOddPage/NextEvenPage now retain
separate zero-width OddPageBreak/EvenPageBreak story points and history snapshots.
Native export writes their exact ParagraphBreakType destination. The obsolete
ordinary-page approximation warning is no longer emitted. Public corpus evidence
currently includes one NextPage break; the odd/even cases use specification-based
fixtures, not external application renders.

The [public break-character reference](https://helpx.adobe.com/indesign/desktop/format-and-style-text/composition-and-text-wrapping/paragraph-break-options-in-indesign.html)
specifies a later numbered page of matching parity with a threaded frame. Each
explicit break remembers its own originating page until that destination is
reached, independently of source offsets and the prior painted text. Consecutive
breaks therefore each advance, including before the first character or an empty
paragraph. Section restarts select parity. Unavailable destinations leave source
or the terminal empty paragraph overset. An intervening Clip frame terminates
the thread even when a page destination would otherwise skip that frame; it owns
the overset finding in both preflight paths. Text edits, snapshot serialization and
undo retain break identity and refuse replacements across protected breaks.

That origin travels with each trial cursor through balanced columns and whole or
split notes. Path containers use the same transition. Focused properties cover
these rules and repeated native saves; independent plate controls cover every
destination, one/two columns, both story directions and numbered sections.
Full native application placement agreement remains unverified.

The combined paragraph-start/explicit-break proof now contains 48 pages. Native
and independent control plates match at three resolutions; both Poppler extracted
samples and rendered pairs agree exactly. The 24 new pages pass visual inspection,
and the first 24 match the previously visually reviewed proof pixel-for-pixel.
The full roadmap sweep passes; current counts and GUI limitations are in Handoff.


### Native No Break ranges

The public IDML schema defines NoBreak as an optional boolean on paragraph and
character styles/ranges. The [public CharacterStyle DOM](https://developer.adobe.com/indesign/uxp/dom/api/c/character-style/)
and [word-break guide](https://helpx.adobe.com/ca/indesign/desktop/format-and-style-text/composition-and-text-wrapping/control-hyphenation-and-word-breaks.html)
define its purpose: selected text stays on the same line. The seven public
Customer’s Canvas packages and ten public OAC/Penn State packages contain only
explicit false defaults; enabled cases are
specification-based fixtures, without an external application placement claim.

Named and local NoBreak values now retain independent inheritance and explicit
false resets through repeated native saves. Local formatting is lowered once;
styles and source text stay stable. NoBreak accepts the XML Schema boolean
literals true/false/1/0 and surrounding XML whitespace, then writes canonical
true/false. See [XML Schema boolean](https://www.w3.org/TR/xmlschema-2/#boolean).
Invalid booleans use the existing localized
invalid-preference warning. Older snapshots omit the optional property and inherit.

Both text-engine wrapping paths suppress automatic breaks inside continuous
enabled ranges, including across independently styled runs. The property does not
split shaping items, change glyph metrics or insert replacement source characters.
Explicit line/paragraph/container breaks remain effective. Layout threads seek a
frame with room for an unbreakable line; unavailable destinations remain overset
and are reported by both preflight paths. Tests cover varying measures, writing
axes, reading directions, UTF-8 carets, complete-story edits and false resets.
Dictionary hyphenation remains separate work. Discretionary-character rendering
is implemented in the following checkpoint.

Character and Paragraph expose a compact toggle and inherit reset under Advanced
Typography. Captured edits undo once; no-ops leave history unchanged. The short
label is present in all 150 catalogs. PSD’s private editable representation retains
the engine setting; its unsupported native subset and Affinity’s unsupported native
subset use their existing pixel/reported raster fallback instead of emitting plain
native text that would wrap differently.

An independent 24-page output proof places ordinary text frames at the protected
thread’s destination. Process/spot plates agree at 72/144/216 dpi across paragraph
and character settings, reading directions and horizontal/vertical writing.
The proof is included in `make check-design-output`; current PDF/UI review and
full-sweep status are in Handoff.

### Discretionary hyphens

Public references: [Unicode UAX #14, section 5.4](https://www.unicode.org/reports/tr14/#SoftHyphen)
identifies U+00AD as an invisible discretionary break and describes language-dependent
visible forms. The [InDesign user guide](https://helpx.adobe.com/ca/indesign/desktop/format-and-style-text/composition-and-text-wrapping/control-hyphenation-and-word-breaks.html)
describes manual discretionary hyphens appearing at selected line ends. No Adobe
headers, binaries or INDD specimens were read for this change. None of the seven
repository IDML fixtures or ten previously downloaded public IDML templates
contains U+00AD, so enabling examples remain specification-based fixtures.

Both engine paths now hide unused U+00AD without adding tracking or interrupting
legacy Latin kerning. A selected break measures and paints a hyphen-minus using
the discretionary character's font/style and original cluster. Break selection
includes that glyph's width: a hyphen that cannot fit cannot manufacture a shorter
overflow line. Leading discretionary characters create no standalone line, explicit
line ends leave them hidden, and No Break suppresses optional breaks. The existing
ordinary overlong-word behavior remains unchanged.

The selected glyph follows the preceding word's resolved bidi level, including
Latin inside RTL paragraphs and Hebrew inside LTR paragraphs. Visual proof review
found that comparing with a bare terminal hyphen in the same paragraph direction
was an inadequate reference: both moved the Latin hyphen before its word. The
corrected independent control uses the word's own direction. This is Schist's
typographic policy, not a Unicode conformance claim or verified InDesign behavior:
the [Unicode discussion](https://www.unicode.org/mail-arch/unicode-ml/y2014-m04/0010.html)
describes the word-direction placement expectation, while
[another reply](https://www.unicode.org/mail-arch/unicode-ml/y2014-m04/0007.html)
explicitly notes that placement is not prescribed by the bidi algorithm.
Hidden-hyphen properties also cover Arabic joining and Devanagari shaping.

Composition retains source byte ranges across boxes and bounded paths. Measured
line spans carry the explicit discretionary-break decision into composed lines;
paragraph-final status cannot substitute for this state, since an explicit newline
inside a paragraph must leave U+00AD hidden. A transient, nonserialized TextSpec
flag carries the decision into isolated line painting, including projected footnote
lines, so preview and separation measure the same glyphs. Carets remain source UTF-8 boundaries; story text is never replaced
with visible hyphens. Native literal and decimal/hexadecimal XML character references
retain styled ranges through four repeated saves.

Independent source-frame controls cover selected and unused hyphens, tracking,
horizontal/vertical flow, reading direction, process and spot inks, and threaded
frames. The output target includes a paired 24-page proof. Dictionary-based
hyphenation, language-specific spelling substitutions/hyphen forms and external
native application placement agreement are not claimed by this change.

### XML boolean spellings

The public RNC declares the supported document, layer, geometry, text and style
switches as xsd:boolean. [XML Schema Datatypes §3.2.2](https://www.w3.org/TR/xmlschema-2/#boolean)
permits `true`, `false`, `1` and `0`; its fixed whitespace facet accepts surrounding
XML space/tab/CR/LF. A package-level property reproduced numeric values becoming
absent style options, with similar direct-string checks in facing pages, locks,
visibility, paths, balancing and object-style category diagnostics.

All typed boolean readers now share the same strict parser. Existing absence,
invalid-value defaults and diagnostic routes remain; non-XML whitespace, uppercase
spellings and other numbers are rejected. Export still writes canonical true/false.
The parser is called only for typed settings, not to normalize arbitrary text,
string attributes or retained opaque XML. Properties compare named styles,
preferences, numbering resources, footnotes, page/parent visibility, guide/layer/
group locks and native geometry through repeated saves in both polarities.
Literal names and story text remain strings. No new UI strings, Adobe headers,
proprietary executables or INDD bytes are involved.

### Native automatic-hyphenation policy

The public IDML text-attribute tables and RNC describe independent minimum word
length, letters before/after a break, capitalized/last/column-end word permissions,
consecutive-line limit, zone and weight. See the
[public ParagraphStyle DOM](https://developer.adobe.com/indesign/uxp/dom/api/p/paragraph-style/)
and [composition guide](https://helpx.adobe.com/ca/indesign/desktop/format-and-style-text/composition-and-text-wrapping/control-hyphenation-and-word-breaks.html).
The minimum word length is inclusive despite the native name
`HyphenateWordsLongerThan`; zero ladder limit means unlimited.

`ParagraphStyle.hyphenation` retains these nine optional settings independently
of the existing `hyphenate` enable switch. Missing settings inherit without
inventing native defaults. A disabled paragraph still retains its policy. Native
paragraph-local overrides lower to reusable styles; save emits the corresponding
standard attributes without private labels. Older Schist snapshots deserialize
with an empty policy. Invalid values receive the existing localized preference
diagnostic; invalid authored values are diagnosed and omitted from native output.

There is a published contradiction: the IDML prose and current DOM document
`HyphenWeight` as 0–100, while the older RNC says 0–10. The codec preserves
0–100 unchanged, following the two descriptive sources. It does not silently
rescale values or claim validation against the contradictory old restriction.
The other integer ranges follow the RNC: before/after 1–15, minimum word 3–25,
ladder 0–25. Zone is a finite nonnegative distance in points. Numeric readers
accept XML whitespace and signed/zero-padded integers, including negative zero.

Four native package properties cover boundaries, absence, independent overrides,
explicit resets, invalid input/output and repeated saves without style growth or
source changes. Two model properties cover independent multilevel inheritance
and older snapshots. These are retention/inheritance checks, not dictionary
composition or external-application placement claims. Automatic dictionary
selection, paragraph-wide hyphen preference, ladder and column-end constraints
remain composition work. No Adobe headers, proprietary executables or INDD bytes
were read for this change.

### Generated break glyphs versus source soft hyphens

The shared text engine accepts transient caller-supplied UTF-8 grapheme break
opportunities. A disposable projection uses the existing discretionary shaping
path and restores original line ranges, paint ownership, caret positions and tab
anchors before returning. Generated glyphs have their own selected-line flag;
they do not create authored U+00AD, style ranges or undo operations. Their
opportunities and isolated-line display flag are excluded from serialization.
The layout line painter carries the selected decision without inserting text.
This is the rendering/source-mapping prerequisite for automatic dictionaries;
it does not by itself enable dictionary composition or claim native placement
agreement.

Six properties compare generated breaks with independent visible glyphs and real
line edges across axes, word directions, styles, caps and combining marks. A
seventh covers the existing source-hyphen bug revealed by that work: an invisible
hyphen must not interrupt a synthetic-small-cap font run and its kerning. Hidden
hyphens are skipped when choosing shaping items; selected display glyphs retain
their own face. The regression compares exact ink against text with no soft
hyphens. No external format assets or proprietary code were used for this fix.

An eighth property covers Hebrew source breaks and blocking controls. The new
RTL generated-break test exposed Unicode 15 LB21a in `unicode-linebreak` 0.1.5
suppressing Hebrew–SHY–Hebrew. A narrow addition permits that intraword case,
including preceding combining marks, while leaving following joiners, marks,
punctuation and explicit No Break protected. It is consistent with the Hebrew
continuation case in [UAX #14 revision 55](https://www.unicode.org/reports/tr14/tr14-55.html#LB21a),
not a claim that the entire line-break engine implements Unicode 17. The failed
first sweep is retained in `/tmp/schist-generated-hyphen-hebrew-before.log`.


### Dictionary opportunity selection

The layout kernel now has a source-preserving word selector using three reviewed
public pattern sets. Dictionary identities are stricter than shaping tags:
US-English and reformed-German patterns cannot silently replace another region
or spelling system. Full words, per-run language, No Break, normalization and
word policies have property coverage. [Dictionary hyphenation](hyphenation.md)
records sources, licenses, accepted identities and limits. Ordinary composition now
uses the selector through frame, column, path and note flows. Trial-owned histories
carry consecutive limits without consuming rejected trials. Complete-word retries
apply column-end restrictions after keeps; unfit text stays overset. Original
source words determine projected footnote opportunities, excluding candidates that
coincide exactly with generated-reference anchors. No source text is rewritten.
Native policy defaults are resolved only for composition, leaving absent attributes
absent on save. The documented greedy zone/weight policy is Schist's own behavior;
InDesign paragraph-composer placement equivalence remains unverified.


### Ordered nested character-style rules

The public IDML specification, schema 152, table 174 and example 92, stores
`AllNestedStyles` under `Properties` as an ordered list of records. Each record
has an `AppliedCharacterStyle` object reference, a `Delimiter` whose type is
string or enumeration, a signed 32-bit `Repetition`, and an `Inclusive` XML
boolean. `Dropcap` is a delimiter enumeration, not a separate `DropCapStyle`
attribute. The public [NestedStyle](https://developer.adobe.com/indesign/uxp/dom/api/n/nested-style/)
and [NestedStyleDelimiters](https://developer.adobe.com/indesign/uxp/dom/api/n/nested-style-delimiters/)
references agree with that distinction.

The typed model retains record order, unknown enumeration names, literal string
whitespace, explicit no-style rules and unresolved opaque references. Missing
lists inherit as a whole unless `EmptyNestedStyles` explicitly clears them;
explicit empty lists also reset inheritance. Character
style renames update every typed reference in the same undo operation, leaving
unresolved identifiers unchanged. Paragraph-local lists use the existing reusable
style lowering and do not become authored character ranges or rewritten source
text. Malformed lists are diagnosed and rejected as a whole, with an explicit
empty override preventing accidental fallback to inherited rules.

All seven Customer's Canvas fixtures carry `EmptyNestedStyles="true"` without
an `AllNestedStyles` element. The public [native export and inheritance
report](https://community.adobe.com/questions-671/cs6-styles-mapping-895785)
also demonstrates this representation on a child whose parent has nested rules.
The reader accepts all four XML boolean spellings on both named styles and local
paragraph ranges. The writer emits the native empty flag for an explicit reset,
false with a populated list, and neither for inheritance. A conflicting clear
flag takes precedence over a populated list and is diagnosed. Invalid flags are
reported; without a valid list they produce an empty override, preventing an
accidental inherited rule. With a valid list, its records remain recoverable.

The leading canonical `Dropcap` record now applies its named character style to
the requested source graphemes. This subset requires an enumeration delimiter,
repetition one and inclusive true. Positive line/character counts enable the
formatting; the existing enlarged geometry starts at two lines. Zero in either
count disables the initial. When the character count is unset, Schist's legacy
one-character policy now emits an explicit native one for active one-line named
initials as well as enlarged initials; IDML Appendix C defaults that field to
zero. Explicit zero and dormant settings remain unchanged. This follows the public
[drop-cap guide](https://helpx.adobe.com/indesign/desktop/format-and-style-text/character-formatting/apply-drop-caps-text-positioning.html).
Unsupported rules, later Dropcap records and unresolved references remain reported on
import/export and for used paragraphs in Preflight. Entirely no-style lists do
not produce false errors. Unsupported native placement flags remain separate.

Geometry and typed paint consume the same derived character runs. Explicit source
character properties override the initial style, inferred from the documented
[formatting hierarchy](https://helpx.adobe.com/indesign/desktop/format-and-style-text/composition-and-text-wrapping/apply-and-manage-text-formatting.html).
Original first-matching range precedence is retained. Dictionary language/No Break,
ordinary generated-list context, used-font preflight and package/native inventories
receive these effective styles too. Footnotes materialize source prefixes before
inserting reference labels; temporary ranges and aliases never enter the saved
story. Whole/split notes retain source editing and undo behavior.

Independent explicitly styled source controls match every process/spot plate at
72/144/216 dpi and all 16 Poppler proof pages. Tests cover inherited rules, Unicode
boundaries, continuation slices, zero/one/multiple line counts, explicit overrides,
writing modes, whole/split notes, source edits/undo and repeated native saves.
This verifies Schist's shared-renderer integration; native application geometry
and formatting precedence agreement remain unverified. The seven Customer's Canvas fixtures
contain no populated `AllNestedStyles`; schema-derived regression packages cover
the records, and the native fixtures cover empty flags through repeated saves.
Separate regressions first reproduced the complete list disappearing and native
reset flags incorrectly inheriting a parent list. No Adobe headers, proprietary
executables or INDD entries were read for this change.

### Source-derived nested delimiters

The supported ordered prefix also composes `AnyCharacter`, literal character
sets, ASCII `Digits`, `Tabs`, `ForcedLineBreak`, `EmSpace`, `EnSpace` and
`NonbreakingSpace`. The public [delimiter reference](https://developer.adobe.com/indesign/uxp/dom/api/n/nested-style-delimiters/)
identifies the explicit-character delimiters; [NestedStyle](https://developer.adobe.com/indesign/uxp/dom/api/n/nested-style/)
defines repetition and inclusive versus exclusive bounds. The [authoring guide](https://helpx.adobe.com/indesign/desktop/format-and-style-text/text-styles/created-nested-styles.html)
describes applying the rules in order. Missing delimiters consume the remaining
paragraph. An excluded delimiter remains available to the next rule. No-style
rules advance the same source cursor without applying character properties.
The public [InDesign user manual](https://helpx.adobe.com/content/dam/help/en/pdf/indesign_reference.pdf),
printed page 326, defines multiple literal characters as alternative terminators:
`-:?` ends at any of those characters, not that substring. Their order and duplicate
entries do not affect matching. Its digit definition is explicitly `0` through
`9`; other Unicode numeric characters are not counted.

Character counts use source graphemes. Literal matches count scalar occurrences,
but their boundaries expand to the containing grapheme, preserving combining
marks and joined emoji. This is Schist's Unicode policy, not verified native
cluster behavior. Rules restart at each source paragraph, never at a wrapped
line or inserted reference. A leading canonical Dropcap can precede the other
supported rules. Source character overrides retain precedence.

Unknown or invalid active bounds stop the supported prefix. Sentence rules,
unverified Repeat forms and structural delimiters remain unsupported. A no-style
unknown rule can affect other named rules and remains
diagnosed; entirely no-style lists cannot change formatting. Source diagnostics
travel with temporary footnote projections, so suppressing materialized styles
cannot suppress their original error. Empty source spans are consumed too: they
must not acquire generated references or note markers after projection. A regression
first reproduced an excluded leading delimiter's zero-length span enlarging a
footnote number and changing its tracking and No Break. No temporary ranges or
aliases are saved.

Properties compare every continuation slice with authored ranges, retain source
through edit/undo and repeated IDML saves, and compare every process/spot plate
in both separation paths at 72/144/216 dpi. A 24-page independently controlled
proof covers vertical text, direct overrides, affine placement, character sets,
digits and generated labels in whole and actual split notes. Every Poppler pair
is pixel-identical and every page has passed visual review. All 16 roadmap targets,
shared UI, formatting and whitespace checks pass. Native examples of this composed delimiter
subset and external-application agreement remain validation gaps. A separate public
[word-rule example](https://forum.rudtp.ru/resources/nested-styles.2151/) supplies
two populated `AnyWord` records, integrated below. Its original IDML,
source hashes and author-posted screenshots are retained outside the repository;
no redistribution permission is assumed.


### Word-based nested rules

`AnyWord` now composes ordered styles from authored paragraph text. Through
includes the terminating whitespace grapheme; up-to leaves it for the following
rule. Leading or repeated whitespace does not create an empty word. Punctuation
and script changes alone do not split words. Unicode whitespace terminates words
except nonbreaking U+00A0, U+2007 and U+202F; combining sequences remain intact.
Missing delimiters consume the remaining paragraph. This is a bounded Schist
segmentation policy, not language-dependent word breaking or a claim of complete
native Unicode agreement. The public manual defines whitespace termination;
[the author's nonbreaking-space example](https://www.creativetechs.com/2006/08/06/discover-nested-styles-in-adobe-indesign/)
supports treating joined terms as one word. The exact Unicode exceptions and
leading/repeated-space cases remain externally unverified.

LeonidB's [public native example](https://forum.rudtp.ru/resources/nested-styles.2151/)
(DOM 7.5, published 2018-05-18) has two ordered `AnyWord` records, both repetition
one and inclusive true, referencing Regular then Bold. One paragraph-style range
contains five Content elements separated by native Br elements. In the associated
[thread](https://forum.rudtp.ru/threads/grep-om-prisvoit-bold-vtoromu-slovu-v-predlozhenii.70614/)
andrejK’s screenshots show the native settings and second-word formatting. The downloaded IDML SHA-256
is `89f7aaed23821a4c442b11d287c9479d8cf2658c459522c12ab178e951ac3cad`.
Its original stays outside the repository; a synthetic regression uses the
observed XML structure with our own text and metrics. It verifies per-paragraph
restarts, every continuation slice, source preservation and repeated native saves.
The original file is also imported locally through the real codec.

Generated note/reference labels containing spaces do not consume source words.
No-style rules, direct overrides, a preceding initial and whole/split notes use
the same source-derived formatting. Independent explicit ranges cover Unicode,
nonbreaking spaces, excluded delimiters, long counts, vertical writing and affine
placement in all process/spot plates at 72/144/216 dpi. The 16-page PDF proof is
part of `make check-design-output`; local visual verification is recorded in
Roadmap / Handoff. Sentence and structural delimiters remain retained and unsupported;
the supported Repeat subset is described below.


### Repeated nested sequences

The public [user manual](https://helpx.adobe.com/content/dam/help/en/pdf/indesign_reference.pdf),
printed page 325, defines Repeat as looping the last requested number of nested
styles and ignoring any later records. The [public DOM enum](https://developer.adobe.com/indesign/uxp/dom/api/n/nested-style-delimiters/)
and IDML schema name the control `Repeat`. andrejK's [native settings screenshot](https://forum.rudtp.ru/attachments/upload_2018-5-18_12-24-47-png.107233/)
shows a three-rule loop: regular word, bold word, then an unstyled span ending
at a period. This corroborates the behavior, not its XML record representation.
A populated native Repeat XML specimen and external application agreement remain
validation gaps. The existing public downloaded word specimen has no Repeat.

Schist supports a no-character-style Repeat control with a positive count of
preceding supported ordinary rules. The control produces no character span, so
its inclusive flag has no effect. A leading canonical Dropcap may precede the
loop; it cannot itself be repeated as an ordinary rule. Other control references,
invalid counts, loops reaching the initial and unknown preceding bounds remain
diagnosed. Ignored trailing records stay intact through native saves.

Each source paragraph starts a fresh sequence. One-time prefix rules run once;
the selected suffix then repeats while consuming source graphemes. A zero-length
member does not prevent another member from advancing. A full cycle with no
source text or supported control progress terminates without inventing a styled range or discarding text.
Generated reference labels never participate in the cycle. Main/whole/split note
projection suppresses already materialized source rules while retaining original
unsupported-setting diagnostics; temporary ranges and aliases remain unsaved.

Properties cover cycle widths and offsets, no-style spans, all continuation
slices, Unicode graphemes, invalid/ignored records, initial boundaries, inherited
paragraph restarts and source edits with exact undo/redo. Independent explicit
ranges compare every process/spot plate at three resolutions in both separation
paths, including actual split notes. Repeated saves preserve source, ordered
records, rendered specifications and typed paint. The new 16-page proof is part
of `make check-design-output`; visual review and full local checks are recorded
in Roadmap / Handoff.


### Letter-count nested rules

The public [user manual](https://helpx.adobe.com/content/dam/help/en/pdf/indesign_reference.pdf),
printed page 326, describes Letters by excluding punctuation, whitespace, digits
and symbols. The published IDML schema names the enumeration `Letters`. Its full
Unicode policy is not specified, and no populated native Letters record or
external composer agreement has been verified.

Schist counts Unicode Letter scalars (Lu/Ll/Lt/Lm/Lo), using the
[general-category API](https://docs.rs/unicode-properties/0.1.4/unicode_properties/trait.UnicodeGeneralCategory.html)
of the already-transitive `unicode-properties` dependency. NumberLetter and
NumberOther, combining marks, punctuation, whitespace and symbols do not count.
This explicit policy is narrower than Rust's Alphabetic property: a Roman numeral
or standalone vowel mark is not a letter here. A through/up-to cut includes or
excludes the whole grapheme containing the requested letter. Multiple letter
scalars in one grapheme count separately but share the same legal cut boundaries.
A missing requested letter consumes the paragraph remainder.

These rules share the ordered source cursor with no-style spans and Repeat.
Generated footnote labels never consume letters, including when the source span
is empty; an initial or authored character range retains its existing precedence.
Unicode properties compare independent byte endpoints through every continuation
slice. Independent source/control documents cover vertical writing, affine
placement, canonical initials, missing bounds, empty spans and whole/split notes.
Repeated native saves preserve rules, editable source, rendered specifications
and typed paint without exporting temporary aliases. The 16-page letter proof
and local verification are recorded in Roadmap / Handoff.

Sentence rules remain diagnosed. In a [first-person native test](https://community.adobe.com/questions-671/heading-and-text-in-same-line-868756),
internal periods in a numbered heading form one sentence boundary. A second
[native report](https://community.adobe.com/questions-671/nested-styles-help-902306)
shows an abbreviation ending a first-sentence rule early. These observations do
not establish exact quote, punctuation-run or through/up-to endpoints. Counting
every period as a separate sentence would contradict the heading evidence.


### Content processing instructions

The public Penn State academic and literary templates each contain two automatic
page-number instructions (`<?ACE 18?>`) in parent-page stories
`Stories/Story_u120fb.xml` and `Stories/Story_u120cd.xml`. The existing typed note
uses `<?ACE 4?>`. The [public behavioral report by Marc Autret](https://indiscripts.com/post/2025/12/indexmatic3-xml-idml-bug-fixes)
identifies other native control instructions, including end-nested-style, indent,
right-indent and section markers. No proprietary implementation or SDK header is
used. General instruction retention does not depend on guessing an unknown code's
meaning.

The XML parser already kept these instructions and their offsets in decoded
Content. Story decoding discarded every instruction except note-body markers,
and then discarded even those when encountered outside a supported note. Main
Content instructions now become anchored story structures instead; the supported
end-style control below has typed composition data, while other codes stay inert.
Their order and UTF-8 positions survive entities, CDATA, multiple Content chunks,
paragraph breaks and empty marker-only stories, without inserting source bytes.

Each recovery payload contains the exact instruction inside constructed XML
wrappers recording its effective paragraph/character style names and the native
PageNumberType value. These wrappers are recovery context, not original outer
container bytes or native marker output. They distinguish current/next/previous
page-number settings and keep local formatting lowered to named styles. Existing
Schist.StructuredStory.v1 retention preserves this data through edits, undo and
repeated saves. Native instruction changes invalidate stale saved coordinates;
new native markers keep their source positions while old payloads survive with
unknown locations. Instructions inside unsupported outer containers stay inside
that original XML, without duplicate extraction. Supported note-body ACE 4 markers
continue through the existing typed note path.

The existing import/export notice, retained count and missing-structure preflight
error disclose the unrendered content. No native page-number, section or indent composition/export is claimed yet.
The end-nested-style subset is described below. Five new properties cover
source boundaries, order, formatting context, page-number modes, marker-only
stories, exact undo/redo, repeated saves, external edits and the four actual page
markers in the public templates. Existing outer-container byte comparisons still
apply; contextual instruction records compare their exact inner PI bytes. Full
local verification is recorded in Roadmap / Handoff.

### Text variables: native custom text and recovery

The public specification's designmap TextVariable section (printed pages 60–62)
separates shared document definitions from formatted Story instances. Example 60
also shows an instance inside a cross-reference. The public proof fixture
`fixtures/indd/proof/proof.idml` contains eleven definitions and three instances
of `Output Date and Time`, in Story_u6ef, Story_u68a and Story_u623. Their native
cached ResultText is `2015-02-19 @ 11:14PM`. These empty inline elements previously
fell through the story walker and vanished without a diagnostic.

TextVariableInstance now retains its exact XML at the source UTF-8 anchor, with
the same constructed formatting/page-number context wrappers as Content
instructions. Cached values are recovery data, not inserted literal source text.
Multiple instances at one position retain their order. Instances inside opaque
tables, notes or inline frames stay in that outer payload without double counting.
The existing retained-content notice, Story Editor count and missing-structure
preflight error identify the unrendered instances. Native story edits invalidate
old coordinates while preserving both the new occurrence and archived payload.

Definitions live once in LayoutDocument.retained_text_variables, separately from
instances. Exact native XML preserves unknown preferences, mixed content and
resource identities. The kernel does not parse it. The standard document Label
Schist.TextVariables.v1 carries this shared archive through saves; definitions
and unsupported instances remain recovery-only. Newly imported native
definitions replace archived entries with the same nonempty Self, while equal
display names do not merge identities. Malformed/duplicate metadata stays inert
and survives subsequent saves without repeated nesting. Older Schist documents
default to an empty archive.

Seven properties cover every UTF-8 boundary, formatting, variable-only stories,
exact one-step undo/redo, native fixture occurrences and definitions, large shared
definitions, unknown preferences, nested opaque content, repeated saves, external
edits and malformed/legacy metadata. The fixture inventory is independently
checked with Python's XML parser. That initial recovery checkpoint prevented loss;
only the typed custom subset below now composes. Style/link dependencies inside
archived definitions are not yet resolved. Adobe's [variable guide](https://helpx.adobe.com/indesign/desktop/add-and-manage-text/conditional-and-variable-text/create-manage-text-variables.html)
specifies that variable content stays on one line; the [instance reference](https://developer.adobe.com/indesign/uxp/dom/api/t/text-variable-instance/)
identifies ResultText as replacement text. Composition uses typed definitions,
source-position mapping and atomic fitting. Opaque recovery XML stays outside the
kernel, and generated values never become editable source bytes. No Adobe headers,
proprietary executables or INDD bytes were read for these variable changes.

The text engine now has transient grapheme-bounded atomic spans for that fitting
step. They suppress internal wrapping and hyphenation without merging adjacent
spans or changing unwrapped shaping/caret geometry. A distinct transient
inline-object range supplies the additional semantics needed by custom variables:
FSI/PDI isolation, bounded shaping context, object-replacement line-break rules and
exclusion of internal spaces from paragraph justification. Ordinary atomic spans
keep their narrower wrapping-only contract.

Literal CustomTextType definitions now also populate LayoutDocument.text_variables.
Typed main-story controls reference their opaque identities, independently of names
or resource-vector order. Only the published CustomTextVariablePreference / Properties /
Contents type=string subset is lowered. Other preferences, mixed/unknown children,
ambiguous identities and unresolved references retain recovery data and diagnostics.

The writer emits native shared definitions and TextVariableInstance elements, with
PageNumberType=TextVariable and empty ResultText. Source bytes and effective character
styles remain separate. Generated identities avoid other package domains; existing
opaque language/list collisions use the package-wide remapper. The standard document
Label Schist.CustomTextVariables.v1 guards authored identities against native edits,
class changes, ambiguous metadata and new unresolved references. TextVariable itself
has no per-resource Label in the published schema, so none is invented.

Structured-story records carry the exact native bindings used at export. Restoring
those records uses actual native references rather than current resource order.
Unresolved archived IDs are never translated through generated native names: a
regression caught that silently activating an unrelated definition. Legacy instance
payloads can gain typed controls after the story guard agrees. Archived reference
identities are reserved by actual native binding, so externally edited definitions
cannot capture unresolved references even when saved resource identities no longer
agree. Deleted bindings keep their original missing identities. Archived definition
XML never creates a live definition after native deletion. Unknown locations stay
unplaced. Native output remains valid when private story/identity metadata is removed;
this does not establish rendering agreement with another application.

Thirteen additional properties cover shared values, empty/stale caches, same-name
resources, reordered definitions, metadata stripping/corruption, external deletion
and edits, every source boundary, stable inline order, native ID collisions and
one-step source/style edits. Full checkpoint verification is recorded in Handoff.
Main-story literal custom values now project into display runs while retaining
original story bytes and source anchors. Coincident variable and note insertions
keep source structure order. Shared definition edits refresh every instance;
variable-only stories do not depend on footnote preferences. Values remain whole
through wrapping and become overset when they cannot fit. Dictionary selection
sees each object as a boundary between source words. Font inventories include the
combined paragraph/instance request. Zero-width bidi controls no longer acquire
tracking advance.

Active initial/nested-style combinations still need logical-object rule counting.
They remain unrendered, as do note-body variables, missing/ambiguous definitions,
invalid anchors and values containing tabs, forced breaks or directional controls.
Empty literal values resolve normally. Existing retained-structure diagnostics
cover unsupported instances. Last page numbers are described below; other
non-custom types remain integration work. The Type menu and Stories toolbar open
a compact variable manager; it also authors last-page-number definitions.
New/Edit expose a name and literal value only while authoring; Save updates the
shared definition in one undo step. Insert and explicit per-instance removal use
a captured, grapheme-bounded canvas source cursor and reject stale stories.
Story Editor and note-body cursor integration remain open. Definitions
with typed references, including note bodies, cannot be deleted until those
references are removed. Opaque recovery XML remains untouched; it cannot restore
a deleted live custom definition on a later import. Pending drafts are tied to
the document session and original definition, not a display name or list index.
The new plate/PDF comparisons use independently authored
ordinary text through Schist's shared renderer; native application placement
agreement is not claimed. Validation, including native review of the authoring
window in Schist's own development app, is recorded in Handoff.

Further public evidence is available in paged-media/core commit
`ffb7c8713125dc77403ec0983099f74ac2558517`: its
[fixture inputs](https://github.com/paged-media/core/blob/ffb7c8713125dc77403ec0983099f74ac2558517/crates/paged-gen/src/samples/variables.rs)
define a custom Edition variable but leave its instance's cached value empty.
The [five-page native PDF](https://github.com/paged-media/core/blob/ffb7c8713125dc77403ec0983099f74ac2558517/corpus/generated/variables.pdf)
shows the defined value in every footer; the export metadata identifies InDesign
20.0.1.32. The same document has five body pages, with a numbering restart, and
its document-scoped last-page variable renders the final label 3. These observations
support definition evaluation and label semantics, but do not establish narrow-frame
wrapping or interactions with initials/nested styles. Inputs, provenance and the
reference PDF were inspected under `/tmp/schist-variable-native-reference/`,
then removed during the requested machine-handoff cleanup. Reacquire them from
the pinned links above; no third-party implementation was copied into Schist.

#### Last page number

The public specification's Schema Example 20 and Table 28 (printed page 62)
define PageNumberVariablePreference with optional TextBefore, Format, TextAfter
and Scope attributes. Format is one of Current, Arabic, UpperRoman, LowerRoman,
UpperLetters, LowerLetters, Kanji, FullWidthArabic, SingleLeadingZeros or
DoubleLeadingZeros; Scope is DocumentScope or SectionScope (VariableNumberingStyles
and VariableScopes in the enumeration appendix). The specification PDF pinned
above (SHA-256 `c45a0d21…70bc`) was reacquired on 2026-10-03 and read with
`pdftotext`. Every checked-in public IDML package, including the proof fixture,
carries the same native default: `Format="Current"`, `Scope="SectionScope"`,
empty TextBefore/TextAfter.

The reacquired paged-media reference (`variables.rs` SHA-256 `5d1ab018…f22f1a`,
`variables.pdf` `185e3340…818e`, InDesign 20.0) has five pages labelled 1 2 1 2 3
on one parent page. Its footer renders the document-scope Current value as 3 on
every page, the section-scope Current value as 2 on pages 1–2 and 3 on pages 3–5,
and a document-scope UpperRoman value as III. The document value is therefore the
final page's label, not a page count, and a section value belongs to the instance's
own destination page.

LastPageNumberType definitions with explicit Format and Scope, one empty-bodied
preference and only the four published attributes now lower to a typed definition;
absent TextBefore/TextAfter are empty strings. Current, Arabic, Roman and letter
formats render through the existing page-numbering styles. Kanji, full-width,
leading-zero, unknown or absent formats and scopes, extra attributes/children and
literal text with controls stay exact recovery data and unresolved instances.
The writer emits all four attributes with their published spellings and uses the
existing guarded identity Label; external preference edits supersede it.

Composition evaluates a value for the pages a thread pass can occupy. A parent
instance uses its destination page, so footers on one parent show each section's
value. An ordinary thread uses its frames' pages: a section value is rendered only
when every frame lies in one section, and otherwise stays unrendered with the
existing retained-structure Preflight error. A visible section prefix on the
supplying page is not guessed into or out of the value; such values also stay
unrendered. Hidden pages count, as they already do in Schist page numbering.
Dates, file names, running headers and cross-reference variables remain recovery
data. Native placement agreement beyond the reference labels is not claimed.

#### Page numbers, section markers and chapter numbers

The public specification enumerates PageNumberType as AutoPageNumber,
NextPageNumber, PreviousPageNumber and TextVariable but does not list the Content
instruction codes. The pinned paged-media fixture inputs above write the page
number as `<?ACE 18?>` in a range carrying that PageNumberType (absent for the
current page) and the section marker as `<?ACE 19?>`; the public PSU templates
contain current page numbers in the same form. Its native PDF labels the five
pages 1 2 1 2 3 and shows the matching section markers on each parent footer.

Main-story ACE 18 with no, AutoPageNumber, NextPageNumber or PreviousPageNumber
type, and ACE 19 with no or AutoPageNumber type, are now typed zero-width controls.
Other modes and instructions remain recovery data. Native saves write them back
as instructions in their own formatted range; current page numbers carry
`PageNumberType="AutoPageNumber"`. Older recovery-only records upgrade after the
story guard agrees, and external native edits supersede saved story data as for
other controls. Style renames include their formatting in one undo step. The
Text Variables window inserts current page numbers and section markers at its
captured cursor with two icons, each one undo step, and lists them beside
variable instances there for exact removal.

A current page number shows its page's Pages-panel label, including a section
prefix only when the section includes it; a section marker shows the marker text,
which may be empty. Both use the same page context as last page numbers: a parent
instance uses its destination page; an ordinary thread renders only when all of
its frames share one page (or, for a marker, one section).

Next and previous page numbers follow public jump-line guidance
([CreativePro](https://creativepro.com/previousnext-page-number-on-same-page))
and InDesign's PDF of the public `variables` sample. A marker in a frame of a
multi-frame story reports the page of the next or previous frame of its own
thread; in a one-frame story, the page of the next or previous frame in the
thread of the story whose frame its frame touches or overlaps; and its own page
when there is no such frame. The label is the Pages-panel label, as for current
page numbers. The frame a marker lands in depends on composition, so composition
runs again with each marker's frame until none moves (at most two more passes).
The sample's story threaded from page 3 to page 5 (labels 1 and 3) prints
"continued on page 3, previous 1" in its first frame and "continued from page 1,
next 3" in its second, as Schist does. Markers on parent pages or touching
stories that disagree stay unrendered and diagnosed. The Text Variables window
inserts both kinds at the captured cursor.

Running headers (MatchParagraphStyleType and MatchCharacterStyleType) are typed
when their preference states the style, SearchStrategy, ChangeCase and
DeleteEndPunctuation, as InDesign writes them (their defaults are not
published); other forms stay retained recovery data. The style reference is
named after the package's styles are read and written back as a
`ParagraphStyle/$ID/` or `CharacterStyle/$ID/` reference. An instance shows, for
the one page it is set on (a parent instance's destination page or a one-page
thread), the first or last paragraph in the style whose first line is on that
page, or the first or last run in the character style that starts there,
between the literal text before and after. Text is ordered on the page by frame
(top to bottom, then left to right) and line; this ordering is a Schist reading.
A page without a match carries the previous page's value of the same variable,
so on the sample's page 2 the first-on-page header still shows page 1's first
heading, not its last. Stories that themselves show a running header are not
searched. ChangeCase follows the sample's PDF: title case capitalizes the first
character of each space-separated word when it is a letter and lowers the rest
("PART TWO begins (a third heading)" becomes "Part Two Begins (a Third
Heading)"); sentence case capitalizes the first character and lowers the rest;
DeleteEndPunctuation drops final sentence punctuation (. , ; : ! ? and their
full-width forms) but keeps a closing parenthesis. All eight of the sample's
headers on all five pages match InDesign's text. The Text Variables window lists
running headers by kind and style but does not edit them.

Date variables (CreationDateType, ModificationDateType, OutputDateType) are
typed when their DateVariablePreference states a Format, and file-name
variables (FileNameType) when their FileNameVariablePreference states
IncludePath and IncludeExtension; other forms stay retained recovery data.
Every public template's default definitions now type: Creation Date
`MM/dd/yy`, Modification Date `MMMM d, yyyy h:mm aa`, Output Date `MM/dd/yy`,
Output Date and Time `YYYY-MM-dd @ hh:mma` and File Name. Formats use these
codes: y or Y (two give two digits, otherwise the full year), M (one or two
digits; MMM and MMMM English month names), d, E (EEE and EEEE English weekday
names), h and H (12- and 24-hour), m, s, a (AM or PM, however many), G (AD);
quoted text is literal and '' is a quote. The public `variables` sample's PDF
prints its formats `yyyy-MM-dd`, `MMMM d, yyyy` and `EEEE dd.MM.yy` as
"2026-10-01", "October 1, 2026" and "Thursday 01.10.26", as Schist does for that
date; the dates themselves came from InDesign's session, so only the formats are
evidence. Creation and modification dates are read from the package's XMP
(xmp:CreateDate and xmp:ModifyDate, as elements or Description attributes, time
zone ignored) and written back there; a document opened without a creation date
is created when opened, and a save sets the modification date. The output date
is when the current output began (PDF or package); outside output it is when the
document was opened, and it is never saved. A file name is the document's file
name, with its folder and extension when asked; before a first save only the
name is known, so a path stays diagnosed. An unknown date stays diagnosed.

Every checked-in public IDML package has `ChapterNumberPreference
ChapterNumber="1" ChapterNumberSource="ContinueFromPreviousDocument"` with a
`ChapterNumberFormat` string `1, 2, 3, 4...`. These were previously dropped on
save. The preference is now retained; invalid numbers or sources are reported and
omitted. ChapterNumberType definitions with an explicit published Format lower
like last page numbers. The reference document has no chapter preference, and
InDesign fills application defaults, rendering chapter 1. Schist therefore uses
chapter 1 when the preference is absent, the stored number for UserDefined, and
chapter 1 for the book-relative sources only when the stored number is 1, where a
standalone document's possible readings agree. Other book-relative numbers are
not guessed. Current format needs the observed Arabic format string; explicit
formats do not. Books and other format spellings remain open.


### End Nested Style controls

The public [manual](https://helpx.adobe.com/content/dam/help/en/pdf/indesign_reference.pdf),
printed page 326, and [current authoring guide](https://helpx.adobe.com/indesign/desktop/format-and-style-text/text-styles/end-or-remove-a-nested-style.html)
describe an inserted End Nested Style character ending a rule before its normal
condition is met. The manual also lists an explicit end-character delimiter.
Marc Autret's [firsthand IDML report](https://indiscripts.com/post/2025/12/indexmatic3-xml-idml-bug-fixes)
identifies its XML instruction as `<?ACE 3?>`. No populated native specimen or
external application rendering has yet been verified for this control.

Main-story ACE 3 now carries a typed zero-width control, separate from its exact
recovery XML. Supported ordinary nested rules stop at that source grapheme
boundary; an explicit EndNestedStyle delimiter supports repetition one. Through
and up-to consume the invisible marker without adding source characters. Ordered
no-style rules and Repeat advance over coincident markers. At a tied boundary, a
through-rule that has already consumed its text delimiter leaves the following
marker to the next rule; an excluded text delimiter follows the marker. These
ordering details are a bounded Schist policy pending native validation.

Rules derive from original paragraph coordinates before generated footnote
references are inserted. Paragraphs restart independently; authored character
overrides retain precedence. Marker anchors follow source edits and exact undo.
Renaming a character style updates the marker's formatting reference in the same
operation. Composition never parses recovery XML. Active initial geometry,
interior-grapheme or unknown anchors and controls inside note bodies remain
explicitly unsupported; end-delimiter counts above one remain diagnosed.

Native saves emit supported controls once at their saved UTF-8 positions with
their character-style references, even without Schist metadata. The guarded
record still preserves original contextual XML. Older inert ACE 3 records upgrade
only after the old native body agrees with their saved model. Native deletion or
movement invalidates stale coordinates instead of resurrecting the old marker.
Unknown instructions and containers keep their existing recovery path.

Properties cover every source grapheme, ordered/coincident markers, repeated
sequences, continuation slices, paragraph restarts, edits, style renames, legacy
records, repeated saves and native deletion. Independent explicit ranges check
all process/spot plates at three resolutions through both separation paths,
including horizontal/vertical text, affine placement and generated references
with whole/split notes. The paired 16-page proof is part of check-design-output.
Local verification and its limitations are recorded in Roadmap / Handoff.

### Text wrap

Page items keep their own TextWrapPreference: TextWrapMode (`None`,
`BoundingBoxTextWrap`, `Contour`, `JumpObjectTextWrap`, `NextColumnTextWrap`),
TextWrapSide (`BothSides`, `LeftSide`, `RightSide`, `SideTowardsSpine`,
`SideAwayFromSpine`, `LargestArea`), Inverse, ApplyToMasterPageOnly, the four
TextWrapOffset distances (Top, Left, Bottom, Right) and ContourOption
(ContourType, IncludeInsideEdges, ContourPathName). Text frames keep
TextFramePreference.IgnoreWrap, layers keep IgnoreWrap, and TextPreference keeps
AbutTextToTextWrap, ZOrderTextWrap and JustifyTextWraps. Spellings and meanings
follow the public IDML specification and Adobe's published text-wrap
documentation; every public fixture uses TextWrapMode="None", and the pinned
paged-media `text-wrap` sample's native PDF never reaches its obstacles, so no
native wrap geometry was available as a reference.

An item's record is written after its own Properties, in source items and in
parent spreads. A record that wraps nothing survives as written. Invalid
spellings, booleans and offsets that are not finite or exceed 10,000 points are
reported and read as the published defaults (no wrap, both sides, false, zero).
`$ID/` is the empty contour path name.

An ObjectStyle's Text Wrap & Other category is now read and written:
EnableTextWrapAndOthers and the style's own TextWrapPreference, inherited through
BasedOn like the other categories. An item without its own TextWrapPreference
takes its style's wrap when the category is enabled; an item's own record,
including one that wraps nothing, wins. Editing a styled item's wrap stores it
only where it differs from the style's, so "no wrap" on an item whose style wraps
is kept as the item's choice and returning to the style's wrap inherits again.
The category's other member, Nonprinting, is not modeled, so a style enabling
the category with Nonprinting true is still reported as having unsupported
categories. Layers' IgnoreWrap now has a toggle on each Layers panel row.

Composition works in each text frame's untransformed composition box. Obstacle
outlines are mapped there through the inverse of the frame's affine, so rotated or
skewed frames and objects wrap in page space. Bounding-box wrap uses the item's
page-aligned bounds plus the four offsets. Contour wrap uses a shape's path, a
graphic frame's clipping path or a text frame's outline (otherwise the frame),
flattened to 0.25 pt, plus the top offset; Schist pushes text from the outline's
extent in each line band, so text does not enter concavities or holes. Inverse
contour and bounding-box wraps keep text within the outline's narrowest interior
across the band, inset by the offset; text below an inverse outline is overset.
Jump-object resumes below the item; next-column ends the column at the first band
reaching it. Sides apply per band, except Largest Area, which is decided once per
column from the item's position so irregular outlines cannot switch sides.
Facing-page spine sides follow the page's position in its spread.

Each free interval of a band becomes one measure for the existing per-line
measure engine, so lines fill both sides of an object in reading order (reversed
for right-to-left paragraphs). A paragraph's band heights feed back into the plan
until the heights composed match the heights planned; a line is never placed into
a band its actual height no longer leaves free. Intervals narrower than the first
line's height are not offered, and an interval an obstacle narrowed that its line
cannot fit (a word wider than the room) is skipped and the paragraph re-planned,
so text never overflows into a wrap. With AbutTextToTextWrap (default true) text
resumes at the next whole leading increment below an obstacle, otherwise at its
edge. ZOrderTextWrap restricts wrap to text frames stacked below the item.
Hidden items and items on hidden layers do not wrap text. Items owned by another
page of the same spread wrap text on this page, so artwork crossing the gutter
wraps both pages. ApplyToMasterPageOnly parent items do not wrap document pages.
JustifyTextWraps is retained; Schist does not justify beside wraps.

Vertical text, enlarged initials, generated list markers and text on a path are
composed unwrapped where an obstacle reaches them, and Preflight warns. Pixel
contours (PhotoshopPath, DetectEdges, AlphaChannel) and IncludeInsideEdges use the
item's outline and Preflight warns that the contour is approximated. Anchored and
inline objects do not wrap yet.

### Text in shaped frames

A TextFrame whose PathGeometry is not its rectangle keeps that path, normalized to
the frame, as its outline, and composes its text inside it. The outline is an
inverse wrap of the frame's own shape in its untransformed box, offset inward by
the frame's top inset (InDesign offers a single inset for non-rectangular frames)
and its stroke's reach, so it shares the text-wrap bands: each line takes the intervals the outline leaves
across its whole height, holes follow the path's fill rule, and other items' wrap
still applies unless the frame ignores wrap, which never removes its own shape.
Text the shape cannot hold is overset and threads on. Vertical text, initials and
list markers in a shaped frame compose in its rectangle with the Preflight wrap
warning. IDML frame geometry carries no fill rule, so saving an even-odd outline
warns and its holes read back under the nonzero rule. "Text in shape" turns a
selected closed shape into such a frame in one undo step, keeping its paint; a
rectangle becomes an ordinary frame.

### Root styles

Every IDML document defines `ParagraphStyle/$ID/[No paragraph style]` and
`CharacterStyle/$ID/[No character style]`, and unstyled text names them. Schist's
empty character style is the root no-style: unstyled runs, controls and list
markers save with the root reference, Styles.xml always defines both roots (as
bare definitions when the document has no style of that name), and every style a
saved story or style names resolves inside the package. Previously unstyled runs
named the undefined `CharacterStyle/$ID/`. On reading, the root character style
and a bare root paragraph style are not document styles; references to the root
character style, including BasedOn, mean no style. InDesign's root paragraph
style carries real defaults and stays a document style, as before.

### Inline anchored items

A Rectangle, Oval, Polygon, GraphicLine, TextFrame or Group inside a story keeps
its exact XML as a retained story structure at its character position, as before,
and is now also typed from that XML: every AnchoredObjectSetting attribute the specification
defines (AnchoredPosition, AnchorPoint, Horizontal/VerticalAlignment,
Horizontal/VerticalReferencePoint, AnchorXoffset, AnchorYoffset,
AnchorSpaceAbove, SpineRelative, PinPosition, LockPosition) with its Appendix C
default when absent (`InlinePosition`, `BottomRightAnchor`, `LeftAlign`,
`TextFrame`, `TopAlign`, `LineBaseline`, zero offsets, pinned), and the page item
itself, read by the same reader as spread items (paint, path or clipped image).
An invalid position or offset is reported and the item stays untyped; an invalid
reference, alignment, anchor point or flag is reported and read as its default.
Items are saved in Schist's structured-story retention record (typed, with the
retained XML), not as native story content, so editing them is not offered and
other applications do not see them; reopening restores them typed. Placed items
take a reserved object id no document object has, so an id saved in another
session never makes an anchored frame compose as a document frame.

An anchored TextFrame is typed with the package story its ParentStory names, by
document position, so it survives saves although story ids are renumbered. Its
story composes in the item's own box as a one-frame thread (threading
attributes are not followed), its own anchored items included, and text it cannot
hold is reported as overset under the item's name. An anchored frame whose story
leads back to the story holding it, directly or through other anchored frames,
is reported as an invalid ParentStory and left untyped, and composition never
sets such a frame. An anchored Group is flattened like spread groups: nested
groups compose their ItemTransforms, opacity multiplies and hidden groups hide
their items. Its members move together, its extent spans theirs, and the
container keeps the group's name and TextWrapPreference.

An inline item in horizontal text is composed as an isolated object replacement
character set as an empty box the width of the item's visual extent: its frame
grown by half its stroke, through its own affine. The extent's bottom sits on the
baseline, raised by the Y offset. Under font-metric leading a tall item raises its
line through the same metrics as a large glyph; under Auto leading the item's line
steps by the item's height above the baseline plus the text's own extra leading
(Auto leading less the point size), never less than the text's Auto leading; fixed
leading keeps its step and a tall item overlaps the line above. A bounding-box
wrap on the item adds its left and right offsets beside it; its top and bottom
offsets change nothing. It wraps whole, like a word. Output and the canvas draw the
item with its frame, after the frame's text, through the frame's affine; a click on
it selects the frame; an anchored frame's text draws as generated text of the
holding frame, so clicking it never edits that frame's story. Composed items no
longer count as unrendered story structures. Items in vertical text are not
composed and stay reported by Preflight; so are contour, jump and column wraps of
inline items, as "wrap not applied to some text".

Native evidence: the public paged-media `anchored` sample
([inputs](https://github.com/paged-media/core/blob/ffb7c8713125dc77403ec0983099f74ac2558517/crates/paged-gen/src/samples/anchored.rs),
SHA-256 `6f8d6915…d0d05266`, and its
[InDesign 20.0.1 PDF](https://github.com/paged-media/core/blob/ffb7c8713125dc77403ec0983099f74ac2558517/corpus/generated/anchored.pdf),
SHA-256 `38c57c7a…a1e574de`) sets a 60 × 36 pt text frame stroked 0.5 pt inline
in 12 pt Open Sans under Auto leading with AnchorYoffset 0. The PDF's content
stream shows the anchor line 38.9 pt below the previous baseline (36.5 + 14.4 −
12) and the next line 14.4 pt further, the stroke's outer edge on the baseline and
at the pen position, and the following text starting at the stroke's right edge.
On the page whose frame has a 3 pt bounding-box wrap, the frame moves 3 pt right,
the following text 6 pt, and no baseline moves. A nonzero inline Y offset is not
covered and remains a Schist reading. The sample's anchored item is a text frame,
which Schist does not type yet, so the composition tests reproduce its geometry
with a stroked rectangle.

### Above-line and custom anchored positions

An item above the line sets its anchor as an empty, zero-width box that keeps room
above its line: its AnchorSpaceAbove, its visual height and its AnchorYoffset
(the space after it) are added to the line's step, so the line and everything
after it move down; the line's own leading is unchanged. The item's stroked
bottom sits its Y offset plus half the em plus half the cap height (of the anchor
character's font) above the lowered baseline. Several items above one line stack
in anchor order. HorizontalAlignment
places it at the left, center or right of the line's column, ignoring paragraph
indents (the specification's "text area"); TextAlign follows the paragraph's
alignment, and SpineRelative swaps left and right on left-hand pages.

An item at a custom position sets its anchor as an empty, zero-width box and
takes no room. Its AnchorPoint (one of nine points of its stroked extent) is put at
a reference point: horizontally the anchor's pen position (AnchorLocation) or the
left, center or right (HorizontalAlignment) of the line's column (ColumnEdge),
the frame (TextFrame), the page margins or the page; vertically the anchor line's
baseline, cap height, x-height or ascent (the anchor character's font: OS/2
sCapHeight and sxHeight, 0.7 and 0.5 em when undeclared), its top of leading (the
baseline less the line's step), or the top, center or bottom (VerticalAlignment)
of the column, frame, margins or page. AnchorYoffset moves it down;
AnchorXoffset moves it away from the side it is aligned to (left for LeftAlign,
right otherwise). SpineRelative mirrors the alignment and anchor point on
left-hand pages. With PinPosition, an item relative to its line is kept between
the frame's top and bottom. Positions are computed in the frame's own space and
drawn through its affine; page rectangles enter that space through the frame's
inverse affine, which is exact for upright frames.

An item at a custom position with a TextWrapPreference wraps the lines of its own
story after its anchor's line: from that line's bottom in the anchor's frame, and
wholly in later frames of the thread on the same page; never the anchor's line or
the lines before it. Each item enters the frame's wrap field like a page item
(its mode, side, offsets and inverse; a group's container carries its wrap), and
composition runs again with the items where the previous pass put them until
they stop moving (at most four further passes), since a wrap can move a later
anchor. Frames of other stories that an item's wrap reaches are not wrapped and
get "wrap not applied to some text"; parent-page instances do not apply anchored
wrap. The rule follows Adobe's public help and tutorials
([CreativePro](https://creativepro.com/wrapping-text-around-an-anchored-object/),
[Adobe community](https://community.adobe.com/t5/indesign-discussions/text-wrapping-stops-when-anchoring-image/m-p/13480287)):
an anchored object's wrap affects the lines after the anchor marker's line, not
that line or those before it. No public native PDF covers it.

Native evidence, from the same sample's PDF content stream (12 pt Open Sans, Auto
leading 14.4 pt, frame at x 67.638–527.638 from y 80, item 60.5 × 36.5 pt
stroked):

- Above line, CenterAlign: the anchor line is 50.9 pt (14.4 + 36.5) below the
  previous baseline, the item is centered on the frame, and its stroked bottom is
  10.2832 pt above the baseline, which is (12 + 8.5664) / 2 for Open Sans's cap
  height of 1462/2048 em. That split is the one sample of it; the space before and
  after, other fonts and the stacking of several items are Schist readings.
- Custom, AnchorLocation, LeftAlign, TopLeftAnchor, AnchorXoffset 24,
  AnchorYoffset 12: the stroked top left is 24 pt left of the anchor's pen
  position (281.611 → 257.611) and 12 pt below its baseline, and the anchor takes
  no width. Only LeftAlign is covered, so the direction of X offsets for other
  alignments is a Schist reading.
- Custom, TextFrame, RightAlign/TopAlign, TopRightAnchor: the stroked top right
  is at the frame's top right. With VerticalReferencePoint LineBaseline the top is
  at the anchor line's baseline; with TopOfLeading it is at the previous baseline
  (14.4 pt above).
- Custom, PageMargins, RightAlign/BottomAlign, BottomRightAnchor, margins 36, 60,
  48, 54: the stroked bottom right is at the margin box's bottom right.
- The sample's VerticalReferencePoint values `AnchorLocation`, `LineCapHeight` and
  `LineXHeight` are not in the specification (which spells `Capheight` and
  `LineXheight`), and their pages match the LineBaseline page exactly, so InDesign
  read them as the default. Cap-height, x-height and ascent references are
  therefore not covered natively.
- Pinning, spine mirroring, column references and the page edge are not covered
  natively; they follow the specification's descriptions.

### Tables

A Table inside a story keeps its exact XML as a retained story structure, saved
in Schist's retention record as before, and is now also typed from it: header,
body and footer row counts, rows (SingleRowHeight, MinimumHeight, MaximumHeight,
AutoGrow), column widths, and cells (`column:row` name, RowSpan, ColumnSpan,
FillColor and FillTint, each edge's StrokeWeight, StrokeColor and StrokeTint,
insets in InDesign's `TextTopInset` spelling or the specification's `TopInset`,
and VerticalJustification; JustifyAlign is read as top). Each cell's paragraphs
become a story of the document, with local formatting lowered to styles as any
story's is, so the Stories panel lists them and the Story Editor edits them. A
table whose counts, sizes or cells disagree is reported and left untyped. Absent
cell insets are 4 pt and absent edges 1 pt black, as InDesign's PDF of the public
paged-media `tables` sample draws its default cells.

A table is set on lines of its own, as blocks the size of its rows, its outer
stroke edge at the line's top left, so a table at a frame's top starts at the
frame's top left as in that PDF. Text before the table in its paragraph ends the
line above it and text after it starts the line below, so a table never shares
a line. Grid lines sit half the outer stroke weight inside the table's edge. A
row that grows is as tall as its top inset, plus its tallest cell's last
baseline below the cell's content top, plus its bottom inset, never less than
MinimumHeight nor more than MaximumHeight; a fixed row keeps SingleRowHeight and
its cells' extra text is overset. InDesign's PDF of the public `tables-rows`
sample (12 pt Open Sans, auto leading, 4 pt insets) has rows of 20.826 pt for
one line and 49.626 pt for three: 4 + 12.826 + 2 × 14.4 + 4, so descenders below
the last baseline are not counted. A cell spanning rows adds what it still needs
to the last of them that grows. Cell text sits below the top inset, moved down
by the spare height for center or bottom justification. Fills cover the cell's
grid area; horizontal edges run the whole cell and reach the outer stroke edge
at the table's sides, and vertical edges stop at the horizontal strokes, as the
`tables` PDF draws them. Output and the canvas draw the table with its frame;
its cell text is generated text of the holding frame on the canvas. A table's
lines step from the line above by the table's height plus the text's extra
leading (its leading less its point size), under fixed leading too, so a table
never overlaps the text above it: a Schist reading, as the samples set each
table alone in its paragraph.

#### Breaking tables

A table that does not fit the room left breaks between whole rows into parts,
each on a line of its own, the next starting at the top of the next column of
the thread. The public [`tables-rows` PDF](https://github.com/paged-media/core/blob/ffb7c8713125dc77403ec0983099f74ac2558517/corpus/generated/tables-rows.pdf)
(SHA-256 `6cd62fdc…bf194`) and its
[generator](https://github.com/paged-media/core/blob/ffb7c8713125dc77403ec0983099f74ac2558517/crates/paged-gen/src/samples/tables_rows.rs) show the rules on
the frame-break pages, each with two threaded 250 pt frames side by side:

- A header row and ten body rows of two or three lines: the first frame holds
  the header and rows 1 to 5 (row 6 would end 11.48 pt below the frame), the
  second repeats the header and holds rows 6 to 10.
- KeepWithNextRow on rows 3 to 6: no break may fall after them, so the first
  frame stops after row 2 and the second holds the header and rows 3 to 7;
  rows 8 to 10 are overset.
- Two 150 pt frames and a 179 pt row: the row before it fills the first frame,
  and the tall row and the row after it are overset, leaving the second frame
  empty.

The public [`tables-overset` PDF](https://github.com/paged-media/core/blob/ffb7c8713125dc77403ec0983099f74ac2558517/corpus/generated/tables-overset.pdf)
(SHA-256 `6d311480…c1959`) and its
[generator](https://github.com/paged-media/core/blob/ffb7c8713125dc77403ec0983099f74ac2558517/crates/paged-gen/src/samples/tables_overset.rs) set four 28 pt
rows of three 120 pt columns in a 360 pt frame of varying height: a 62 pt frame
shows two rows, a 20 pt frame none, and with a header row a 30 pt frame shows
nothing although the header alone would fit, so a part never stands without a
body row. Its 361 pt table overhangs the frame by its right stroke, so a table
wider than its column is still set, overhanging it.

Each part shows the header rows unless it is the first and SkipFirstHeader is
set, and otherwise as BreakHeaders says: in every column (InAllTextColumns,
the default), or only in the first part of each frame (OncePerTextFrame) or page
(OncePerPage). Footer rows close each part the same way by BreakFooters, and the
last part unless SkipLastFooter is set; a table that fits shows both unless
skipped. A break never falls inside a cell spanning rows. Kept rows give way only
in a column nothing else would fill: there the table breaks after the last row
that fits. A row no column holds leaves it and every row after it overset in one
last part, which the frame reports as overset text. Where the parts fall depends
on where composition sets them, so a story holding tables is composed again
until its parts settle. The first part fills the room below the line before the
table, measured where it was set; when a part moves on past the column planned
for it, that column is taken as too short for it and the parts are planned
again.

A body row's StartRow other than Anywhere ends the part before it, kept rows
above it included, and starts its part at the top of the next column, the next
frame, the next page or the next odd- or even-numbered page, as the
specification describes and as a paragraph's StartParagraph is composed. The
table's first row, or its first body row under header rows, moves the whole
table on from the text before it; a table with nothing before it stays where it
is. No public sample sets one. A StartRow on a header or footer row below the
first, or on a body row a cell spans into, is reported and not applied, and so
are StartRow, BreakHeaders and BreakFooters values the specification does not
define.

#### Table and cell styles

A table's settings resolve through its AppliedTableStyle, the styles that one is
based on, and `[No table style]`; a cell's through its own attributes, its
AppliedCellStyle, the cell style its table region gives it, their bases and
`[None]`, with the table's own inset and justification settings last. Regions
follow the table style: the header or footer region's cell style when it is not
the same as the body's, then the left or right column's, then the body's. Styles
link only through the specification's `Properties/BasedOn` element. The public
[`styles-cascade` PDF](https://github.com/paged-media/core/blob/ffb7c8713125dc77403ec0983099f74ac2558517/corpus/generated/styles-cascade.pdf) (SHA-256
`3d95ee32…5b576`, page 3) and its
[generator](https://github.com/paged-media/core/blob/ffb7c8713125dc77403ec0983099f74ac2558517/crates/paged-gen/src/samples/styles_cascade.rs) name the bases of
a table style and of the cell style supplying a fill and centring in a
`BasedOn` attribute instead, and InDesign draws that table with no fill and its
text at the top. The styles are typed when the table is read; the
RootTableStyleGroup and RootCellStyleGroup XML is retained and saved unchanged,
so a reopened table resolves the same.

Fills alternate as the table or its style says: StartRowFillCount rows take the
first fill and EndRowFillCount rows the next, over and over, after
SkipFirstAlternatingFillRows and before SkipLastAlternatingFillRows, and the
same by column. Defaults are those InDesign's own exports write for
`[No table style]`: no pattern, a first fill of black at 20 %, a next fill of
none. Page 10 of the public [`tables` PDF](https://github.com/paged-media/core/blob/ffb7c8713125dc77403ec0983099f74ac2558517/corpus/generated/tables.pdf)
(SHA-256 `3bd73efe…f3a72`) fills rows 1 and 3 of three with its style's 20 %
cyan swatch at that default 20 % tint (4 % cyan). Row patterns skip header and
footer rows and column patterns do not, as Adobe's public table help says.
ColumnFillsPriority false hides column fills and true row fills, as the
specification says; page 11, a style giving only column fills, draws none. A
fill the cell or its cell style gives, none included, wins over the pattern.

#### Borders, strokes and spacing

A table's border, alternating strokes, space before and after and StrokeOrder
resolve through the table, its AppliedTableStyle, that style's bases and `[No
table style]`, with the defaults InDesign's own exports write for `[No table
style]` and the specification's Appendix C gives: 1 pt black borders, 4 pt
before, −4 pt after, no stroke pattern (a first group of 1 pt black and a next
group of 0.25 pt black) and BestJoins. They are resolved into the cells' edges
when the table is read. Each attribute of an edge (weight, colour, tint, type)
comes from the cell or its cell styles; else, on the table's outside, from the
table's border (TopBorderStrokeWeight …); else from the alternating strokes
group its row or column falls in (StartRowStrokeCount, StartRowStroke…,
EndRowStroke…, the skip counts; the specification names the next column group's
type EndColumnLineStyle); else InDesign's 1 pt black. Row strokes count the body
rows and column strokes every column, header and footer rows included, as fills
alternate. Each row or column in a group strokes both its edges, and where two
groups meet the lower row's or right column's is drawn: a Schist reading of the
specification's "row borders", as no sample sets a pattern.

The public [`tables`
PDF](https://github.com/paged-media/core/blob/ffb7c8713125dc77403ec0983099f74ac2558517/corpus/generated/tables.pdf)
(SHA-256 `3bd73efe…f3a72`) and its
[generator](https://github.com/paged-media/core/blob/ffb7c8713125dc77403ec0983099f74ac2558517/crates/paged-gen/src/samples/tables.rs)
show which edge a grid line two cells share draws, and how strokes meet. On page
5 a 3 × 3 table's middle cell states 3 pt magenta edges and its neighbours state
none: all four of its grid lines draw magenta, after the black. The magenta row
strokes run 1.5 pt past the crossings (176.638 to 299.638 for column lines at
178.138 and 298.138) and the black row strokes beside them 0.5 pt (57.638 to
178.638), while row strokes meeting the same stroke end flush at the crossing
(page 1). Column strokes stop half the crossing row stroke's weight short (the
magenta from 627.8663 to 652.8663 between row lines at 626.3663 and 654.3663,
the black at 654.8663), and run to the crossing where no row stroke crosses
(page 6: 418.138 down to 654.3663 beside a cell spanning rows). Strokes are
drawn per grid line and column, a cell spanning columns included (page 6 strokes
its spanned top row three times). On page 12 the line below the header row draws
the header's 1 pt black bottom edge, not the 2 pt magenta top edge at 50 % that
the body cell below it states. So an edge the cell or its styles state wins over
one from the table's border or strokes, which wins over an unstated edge, the
lower or right cell's winning a tie; above the body a header row's bottom edge
is drawn, and below it a footer row's top edge (by symmetry, a Schist reading).
Row strokes are in front, as BestJoins, RowOnTop and Indesign2Compatibility put
solid strokes; ColumnOnTop puts column strokes in front, running through the
crossings while row strokes stop at them (the specification's reading). Output
and the canvas draw the same strokes.

SpaceBefore keeps room above a table's first part, and SpaceAfter keeps room
between its last part and the next line of its column; a negative space before
draws the table up by its leading, never past the text's own leading. A table
starting a column, frame or cell drops its space before. The `tables` PDF sets
its tables' outer edge at their frame's top (the top stroke centred at 682.3663
under a frame top of 682.8663 on page 1) and page 2's nested table at its cell's
4 pt inset (677.8663 under the row line at 682.3663). The public [`tables-rows`
PDF](https://github.com/paged-media/core/blob/ffb7c8713125dc77403ec0983099f74ac2558517/corpus/generated/tables-rows.pdf)
(SHA-256 `6cd62fdc…bf194`) and its
[generator](https://github.com/paged-media/core/blob/ffb7c8713125dc77403ec0983099f74ac2558517/crates/paged-gen/src/samples/tables_rows.rs)
start both parts on page 8 at their frames' tops (721.39 under 721.89), although
`[No table style]` keeps 4 pt before tables. No sample sets text before or after
a table, so the space there follows the specification ("the space above the
table", "the space below the table") on Schist's leading step. Stroke types
other than solid and overprinted strokes, on cell edges, borders and alternating
strokes, and StrokeOrder values the specification does not define, are reported.

Not composed yet: cell styles' paragraph styles, diagonal lines, cell rotation,
stroke styles other than solid, overprinted strokes, and editing tables and their
styles.

# Print separation

`crates/separation` turns a page of Design Mode's layout into printing
plates. A layout says "this shape is PANTONE 032 C at 60%, that
paragraph is black body copy"; separation answers the question a press
operator asks: which inks, at what coverage, on which plates.

## The problem this crate exists to solve

`schist_core::ink` already holds a working spot-plate system: scalar
coverage tiles, DisplayInfo metadata, PSD round-trip, cloud CRDT, undo.
It models a **painted plate** — a buffer you brush ink into, which is
the right model for retouching a channel.

Page layout works on a different axis. An object *names* an ink,
overprint is a *per-object* property, and the plate is *derived* at
output time by asking which inks touched each pixel. Neither model can
express the other on its own.

So both exist. This crate derives plates and then emits them as
`schist_core::InkChannel`, which inherits the registration, undo, CRDT
and PSD write for free. Nothing in `schist-core` was changed to make
this work.

## The stages

| Stage | Module | What it does |
|---|---|---|
| 1 | `plan` | Resolves inks and the manager's rules into an ordered plate list |
| 2 | `raster` | Turns each object into a coverage mask |
| 3 | `coverage` | Accumulates masks onto plates — where knockout and overprint differ |
| 4 | `separate` | Applies UCR, black generation; measures the ink limit |
| 5 | `separate` | Emits `InkChannel` plates |

Beyond that:

| Module | What it does |
|---|---|
| `build` | Where an ink's CMYK build comes from |
| `halftone` | Screening a plate to inked cells, and trapping ink pairs |
| `pdf` | The prepress PDF writer |
| `preview` | The separations preview panel's data and drawing |

## Knockout and overprint

These are the whole of `InkMode`, and the only place in the crate where
the distinction is implemented. It is a property of the *object*, not of
the ink or the plate.

- **Knockout** removes the ink beneath and prints. A knockout reaches
  *every* plate the object covers, not merely the ones it inks: a spot
  mark knocked out over a black box leaves a hole in the black. A rule
  that only touched the object's own plates would turn every knockout
  into a silent overprint, which is the single most expensive bug
  available in this code.
- **Overprint** prints on top. Two objects sharing a plate add their
  coverage, clamped, and a hairline of colour inside a black box does not
  punch through it.

Nothing about this is visible on screen, and nothing about it is
guessable from a preview.

## Where an ink's CMYK build comes from

`Ink::to_cmyk` is the layout model's own conversion, and it is
**preview-grade**: it sets K to `1 - max(r,g,b)`, so one colour channel
always comes out exactly zero.

That is fine for a swatch preview. It is not fine for separation, and
specifically it makes under-colour removal a silent no-op: UCR withdraws
from the smallest channel, and a build with a zero channel has nothing
to withdraw from. The first version of this crate had exactly that bug
and UCR did nothing at all.

So the build is injected. `CmykSource` is the seam, `NamedBuilds` is a
name-keyed table for a prepress provider's spec sheet, and
`separate_page_built` is the entry point. An editor with ICC machinery
should use it.

## Process ink is weighted per channel

A process ink's coat weight is its own channel's share, not a single
tint. A colour whose build is 100/28/100/6 lays 100% cyan, 28% magenta,
100% yellow and 6% black. Weighting all four equally turns a process mix
into a flood of ink, which is both wrong and obvious once you look at the
plates.

## The ink limit is reported, not enforced

`InkManager::total_area_limit` defaults to 300%, a common coated-sheet
figure. Separation does **not** clamp to it by default, because clamping
changes the artwork — and a preflight report that silently fixed the
page would hide the very thing preflight exists to find.

`separate_page_with(.., enforce_ink_limit = true)` opts in, for when the
user has asked for the fix. The default is off and should stay that way.

`InkManager::is_identity` lets a caller skip a full-page pixel pass when
UCR, black generation and the limit are all off, which is the common
case.

## Preflight

`PreflightReport` currently reports missing links, unavailable graphic
pixels (including embedded graphics), and total area coverage. Messages
use `schist-i18n` and the Design catalog. What it does *not* do is guess:
with no limit configured it stays silent rather than assuming 300%, because
crying wolf on every
dark image trains people to ignore it.

`report.is_printable()` rejects reports containing errors. It covers the
checks above, not every possible prepress problem. The Design Preflight
panel runs these checks on demand at 72 dpi and labels coverage as a
preview; it is not an output-resolution proof. Until the editor supplies
placed-image pixels, those graphics produce errors rather than passing
with their coverage omitted.

## Placed graphics

This crate cannot resolve a `Link` — reading a PSD or JPEG is the
editor's job. `GraphicSource` is the seam. With `NoGraphics`, the separation
pass reports known missing links as missing and other non-empty graphics
as unavailable. Both prevent a partial page from passing preflight.

`PlacedGraphic` carries coverage plus a per-pixel CMYK build, so the
caller decides how a linked image separates rather than this crate
guessing.

## Units and the pixel grid

Layout is in points; separation happens in pixels. The conversion goes
through `OutputSettings::to_pixels` and `PagePixel::rect` and nowhere
else. A rectangle's right and bottom edges are derived from its rounded
left and top plus the rounded size, so a plate's width is always exactly
what the resolution implies — deriving each edge independently lets a
plate come out a pixel narrower than its neighbour, which a press
operator notices immediately.

With bleed on, the page origin is **negative**: the paper starts above
and left of the trim. Artwork positions themselves relative to the
plate's own origin, so turning bleed on changes the output box without
shifting the artwork within it.

## Shape geometry

`LayoutObject::Shape`'s points are **relative to the object's bounds
origin**, the same convention a group's children use, and are in points.
Separation scales them into the page's pixel grid before rasterising. A
path left in points would come out 300/72 times too small, and the shape
would look like a hairline in the corner — correctly laid out, correctly
placed, and completely wrong to look at.

## Verification

`make check-separation` runs the tests, `make lint-separation` runs
clippy, `make check-separation-wasm` type-checks for the browser. The
separation tests run with the layout tests because the two have to
agree.

The tests that matter most:

- a knockout reaches plates the object does not ink, and an overprint
  leaves them alone;
- a spot lands on its own plate and contributes nothing to the process
  plates; converting it to process does the opposite;
- two shapes meeting knock out leave a seam and overprinting does not;
- the injected CMYK build is what reaches the plates, per channel;
- UCR moves ink onto the black plate when given a build with a smallest
  channel, and the naive build is documented as unable to;
- composition and separation agree on where the text is;
- a higher resolution puts the same ink on more pixels without eroding
  the edge into a grey fringe;
- a parent page's objects appear on every page it is applied to.

## Prepress PDF

`pdf::write_page` writes rasterized ink channels in a single DeviceN image.
The image retains each process and spot plate. A Type 4 tint function supplies
a CMYK screen alternate; a Separation definition per ink uses a Type 2
function. Zero ink stays zero. PDF names preserve UTF-8 bytes using #XX escapes.

One combined image matters: painting opaque per-plate images made each plate
erase earlier ones in ordinary PDF viewers. Poppler exposed that failure even
though the old string-based tests passed. It also exposed malformed cubic
operands in registration marks. `make check-design-output` now renders a
four-ink proof independently, rejects renderer diagnostics, and checks the
colored patches and paper. The rendered proof was visually inspected too.

The MediaBox includes room for crop and registration marks. BleedBox and
TrimBox lie inside it; marks stop outside bleed. Registration marks use the
PDF All colorant. Marks may be omitted for a screen proof. Object knockout,
overprint and opacity have already been resolved into the plate samples.

`write_document_imposed` and `write_sheet` pack sequential pages 1-, 2- or
4-up using a PDF Form XObject per layout page. Mixed sizes retain physical
size and incomplete sheets have blank cells. This is sequential packing, not
booklet signature imposition. The output window selects resolution, n-up,
marks, hidden pages and an optional CMYK ICC profile. The profile is embedded
as an OutputIntent; this does not certify PDF/X conformance. This remains a
raster plate export, not editable vector/text PDF.

Text separation uses paint masks from a single shaping pass per line. Each
run retains its ink identity, opacity and overprint. Two spot inks sharing an
RGB preview still produce different plates. Process paints sharing a name
retain distinct CMYK builds. Alias chains resolve to the final plate definition
regardless of resource order, including final targets converted to process.
Native CMYK placed artwork bypasses RGB re-separation.

Preflight includes unavailable graphics, missing fonts used by visible text,
terminal overset and effective image resolution below 150 dpi. Passing text
along a thread is not an overset error. Preview checks stay labelled 72 dpi;
output checks use the selected raster resolution. PDF export returns warnings
along with the bytes so the UI can show them.

The encoding follows the public PDF specification, summarized by the PDF
Association's [color functions](https://pdfa.org/download-area/cheat-sheets/Color.pdf)
and [color spaces](https://pdfa.org/download-area/cheat-sheets/CommonObjects.pdf).

## Halftoning

A plate is continuous coverage; a press prints through a screen, a fine
grid of cells each inked or not. `halftone` decides which cells.

The threshold pattern is a **ranked table**: every sub-cell of a 16x16
cell gets the threshold `rank / area` in order of its distance from the
centre. A single radial threshold cannot work — a disc of area `c` needs
a radius of `sqrt(c/pi)` where the naive rule uses `c`, so a mid tint
prints far too light. The ranked table gives round dots *and* an inked
fraction that matches the coverage.

The screen angle is not decoration. Four plates at the same angle print
their dots on top of each other and moire, which is why
`Halftone::process` spaces the process plates 15° apart.

A frequency below the plate's resolution degrades to one cell per pixel,
because sampling finer than the plate can only alias. A zero frequency
does not divide by nothing.

## Trapping

Trapping fills the gap between two abutting objects: if two inks meet at
an edge and the sheet shifts in register, bare paper shows as a white
hairline. Laying a little of one ink under the other closes it.

`trap_order` puts the **lighter** ink under the darker, because the
darker is the edge a printer is watching. Two inks of equal darkness are
ordered by name, so the result is stable rather than arbitrary.

`trap` never widens past the output box, because ink outside the paper is
ink wasted, and it traps the *union* of a touching pair rather than one
plate alone — trapping one plate alone fills its edges rather than the
seam. `trap_all` returns a `Trapping` with both the widened plates and
the pairs that met, so preflight can say which inks touch.

## Separations preview

`schist_core::ink::InkPreview::Separation` replaces the pixel with
greyscale. That is right for a *painted* plate, where there is a
composite to isolate. It is wrong for a page layout, where there is no
composite and the question is "which inks does this page use, and where".

`SeparationsPreview` answers that. It draws a plate in its own ink over
white paper, or over the composite, and `summaries` reports each plate's
peak, mean, solid fraction and a coarse word. The word is by area and
does **not** capture how the ink is arranged: fine line work and a flat
4% wash can share a mean. A panel shows the figures alongside, and the
figure is what a prepress provider actually reads.

The words themselves are deliberately not translated in the kernel — they
are the vocabulary a print buyer uses in a specification. The editor
supplies the localised form.

## Not done

- **Hyphenation.** The text engine has no hyphenation, and a real
  implementation needs a Liang-style algorithm plus per-language pattern
  files, which is a subsystem rather than a feature. `ParagraphStyle::hyphenate`
  is carried for interchange and does nothing yet.
- **Table and footnote structures.** IDML carries them; the story model
  keeps an unknown point opaque so they round-trip, but they do not
  compose.
- **Object-level overprint for graphics** follows the object's flag
  rather than being derived from the flattened artwork. Correct for
  Schist's model; it would need revisiting for imported flattened
  PDF-like content.
- **Ink limits beyond total area coverage**, and no paper-stock or
  press-profile modelling.
- **The spot ICC path.** A spot's Lab is stored, but nothing resolves it
  through a profile to a real ink.

## Page geometry and spread artwork

Four-sided bleed and slug now reach `PageOutput` as physical offsets from trim.
Media encloses both and any required printer marks. PDF's bottom-left coordinates
use the left/bottom media offsets for TrimBox and plate placement; BleedBox is
inset separately on each side. Each crop mark starts outside its corresponding
bleed edge. The uniform-bleed `write_document` convenience API still writes no
slug; `write_sheet` accepts the complete geometry used by the editor. Slug adds
paper for marks; artwork separation remains clipped to the bleed box.

`offsets_proof` and `check-design-output` exercise unequal bleed/slug edges and
single/two-up placement. Independent Poppler parsing and pixel checks distinguish
all four bleed bands, the trim origin, blank slug paper, and PDF page boxes.


Both separation entry points and preflight use spread artwork in the destination
page's coordinate system. Artwork crossing a gutter contributes to both pages;
inside bleed can include adjacent-page artwork. Shapes retain opacity, strokes
and layer stacking; graphics retain native process channels; text composes
in its original frame/grid before placement. Missing graphics crossing into the
requested output area are reported on that page too. Objects belonging to another
spread are excluded.

Property tests compare every output plate pixel against the same region of a
whole-spread reference for two through four unequal pages, both physical orders,
zero/nonzero gutters and two resolutions. The exact raster comparison uses binary
representable shear so coordinate rounding does not introduce one-step alpha
quantization differences. `crossover_proof` independently checks continuous
image/text, transparent overlaps, inside bleed, and identical single/two-up output
with Poppler. Its n-up sheet includes both pages' inside bleeds; those repeated
strips are outside the trim, as expected. This is sequential n-up, not booklet
imposition or an external application's rendering comparison.

## Direct ink tints

Shape fill/stroke and styled text fill tints scale the resolved coat weights.
The full-strength ink definition still identifies a spot plate and supplies an
ICC/process build or alias target. Tint is applied after that resolution; it
never scales the geometric mask. A 0% knockout therefore removes all underlying
ink, while opacity retains the corresponding fraction of underlying ink.
Overprint keeps underlying plates and adds the tinted amounts under the existing
additive model. Both plate coverage and composite preview follow the same rule.

Property tests cover tint/opacity/overprint combinations for fills and strokes,
spot aliases, process conversion, and inherited text paints. The `tint_proof`
example adds an independent Poppler check of process/spot ramps, zero-tint
knockout, transparency, overprint and styled glyphs. Text stroke rendering,
and external native-application comparison remain gaps.


## Decorated text paints

Solid underlines and strikethroughs default to their character's resolved fill
paint, including spot identity, tint, opacity and overprint. Custom line inks,
tints and overprint retain independent native plate identity. Consecutive equal
paints union before opacity; differing inks keep underline/glyph/stroke/strike
order within each visual text run.
Previously the renderer appended all underlines after all glyphs, which could
apply opacity twice at intersections or repaint an earlier run over a later one.
Tests bound every decorated plate pixel by its requested opacity in knockout and
overprint modes, across horizontal and both vertical writing directions.

The paired `decoration_proof` pages let Poppler compare undecorated/decorated
output directly. Checks require added continuous ink through spaces and unchanged
solid glyph colours. The proof also has visually inspected horizontal/vertical
pages. Text strokes and custom solid decoration paints are implemented below.


Explicit text baseline offsets scale from document points to output pixels before
rasterization, independently of leading, tint and opacity. Separation tests compare
whole plates to translated references at three resolutions in horizontal and both
vertical writing modes. The baseline PDF proof pairs unshifted and shifted pages
for horizontal type, mixed upright Japanese/rotated Latin vertical type, both
column directions and a rotated frame. Poppler extracts and compares every image
sample exactly, then checks rendered page placement. This separates image-sample
fidelity from page rasterization. The checks include decorated translucent glyphs,
so a lost offset, wrong sign, unscaled point value or repeated opacity cannot pass
by matching metadata alone.

Page contribution bounds expand for the largest supported absolute baseline
offset referenced by a story, before frame transforms. This conservative envelope
keeps ink crossing a gutter even when its frame remains wholly on the source
page; a two-direction plate comparison checks against explicit placement.


Named tint swatches reuse their full-strength base Color's plate or process build.
A named percentage is applied once, after alias resolution, and owns the paint's
tint even if a stale per-use fraction remains. Spot aliases and conversion to
process behave identically to a direct tint; neither creates a second spot plate.
Tests compare every plate and composite sample against direct-tint references,
including zero-tint knockout. The existing Poppler tint proof now alternates
named and direct process/spot patches and uses a named tint for body text.


## Automatic text positions

The same resolved glyph sizes and nominal metrics reach preview and output.
Document preferences scale superscript/subscript glyphs and derive signed movement
from regular leading, with explicit offsets added afterward. Point-to-pixel and
canvas scaling include nominal metrics; scripts retain regular line spacing.
Conservative page-contribution bounds include the derived shift, so scripts can
cross the gutter while their frame remains entirely on its source page.

The nine-page script proof uses horizontal Latin and both vertical Japanese column
progressions. Poppler extraction checks exact displacement between equal-size
super/subscript images, half-size glyph extents and unchanged spacing between
coloured paragraphs. Every proof page was visually inspected. This does not
establish native application agreement for advanced OpenType positioning.


Ranged OpenType settings share the same shaper in preview and separation. The
`opentype_proof` example emits all-off, all-on and mixed pages in each writing
mode. `check-design-output` compares the mixed page to the appropriate paragraph
from each control using both Poppler image extraction and page rasterization.
This checks actual ligature/kerning changes, inheritance boundaries and unchanged
paragraph placement instead of merely looking for feature tags in serialized data.


## Leading output

Absolute leading scales with output resolution independently of nominal glyph
metrics. The leading proof pairs flowed mixed-size text with separately placed
lines at known baseline or column-center distances, including a blank line.
Fixed and automatic spacing run through horizontal and both vertical modes.
`check-design-output` compares both Poppler-rendered pages and independently
extracted image samples. Conservative story contribution bounds also include
nominal cells extending before a frame under tight leading; a cross-gutter
property compares that ink with explicit placement at three resolutions.


Named static font variants use the same resolver in preview and print. Preflight
checks used family/variant pairs, reports a missing family once, and reports missing
variants of available families separately. Unused styles do not fail preflight.
The nine-page `font_style_proof` compares Regular, Light and mixed ranges in all
three writing modes. Poppler checks lighter glyph density and exact mixed-page
agreement with the controls, in extracted samples and rendered pages. Every page
was visually inspected. The font is an unmodified public IBM OFL fixture.


Object-style and inline frame paints share shape separation. Fill precedes text
or placed pixels and stroke follows them; each retains its own tint/overprint
while using the frame transform and opacity. Stroke bounds contribute across
page gutters. A missing image still produces a preflight error, while the frame's
available fill and stroke remain printable. The object-style proof compares
inherited rectangular/elliptical text and image frames against three independently
placed objects (fill, content, stroke). Curved text flow remains rectangular and
is disclosed by the IDML codec.

Text fill and glyph strokes reach separation as distinct ink-tagged coverage
paints from a single shaping pass. Stroke points scale with output DPI; glyph
metrics and line wrapping remain unchanged. Centered and outside outlines,
independent tints/overprint, explicit no-fill/no-stroke and inherited styles share
the preview model. Repeated equal inks merge before opacity is applied. Stroke
extents also participate in cross-gutter contribution bounds. The
`text_stroke_proof` example compares six pairs of PDF pages across horizontal
and both vertical writing modes, affine placement, spot tint and transparency.
It also covers miter, round and bevel joins, with zero and extended miter limits.
The dimensionless limit stays unchanged during DPI scaling. Contribution bounds
use the resolved limit for miter joins and stroke width for round/bevel joins.

Custom solid decorations scale point weights and offsets before rasterization.
Exact rectangle coverage avoids direction-dependent quantization of thin lines.
They leave font shaping and advances unchanged, remain visible with a separate
ink when glyph fill is disabled, and contribute outside frames across gutters.
The twelve-page `custom_decoration_proof` compares six decorated/reference pairs
using independent underline, glyph and strike objects, including fractional
weights, all writing modes, affine placement, spot tints, opacity and overprint.
Striped decorations, gap inks and unadjusted dashes are implemented
below. Dotted and path decorations are described below.


Mode-dependent CJK kana/proportional defaults are expanded before shaping for
each paragraph's axis, including ranged overrides. Plate checks cover all three
writing modes, affine frames and multiple resolutions. The nine-page CJK proof
compares mode defaults against explicit feature tags in both extracted samples
and rendered pages. Bundled Noto CJK supplies real horizontal/vertical
proportional adjustments; kana alternate glyphs and native-application agreement
remain unverified.


Capitalization shares the source-cluster-aware shaper used by preview and caret
geometry. Ranged native small caps and expanded uppercase text retain spot ink,
tint and affine placement across output resolutions. The capitalization proof
pairs all caps, native small caps, OpenType all-small-caps and synthetic small
caps with independently authored controls in all three writing modes. Poppler
compares both sample data and rendered pages; all 24 pages were visually inspected.
Synthetic percentages scale glyphs without changing nominal line cells,
automatic decoration metrics or source text. Conservative contribution bounds
include oversized synthetic caps crossing page gutters. PSD/Affinity preserve their existing reported/private or pixel fallback for
unsupported native capitalization; Normal remains native-eligible.


Striped underline/strike paints partition exact pixel area across the line weight.
Gap area complements stripe area before each ink is quantized independently,
including fractional edges. Their 8-bit sum can differ by one rounding level.
Equal paints share a single silhouette before opacity. Separate inks retain
independent spot/process identity, tints and overprint; gap paint precedes line
paint. Glyph shaping, advances and caret positions remain unchanged. Point sizes
scale with DPI, while the stripe definition stays in percentages.

The `striped_decoration_proof` compares twelve pages with independently authored
solid bands. Cases cover both decorations, all writing modes, fractional widths,
affine transforms, opacity, spot tints and opposite line/gap overprint settings.
Kernel properties also check subpixel stripe area and transposed horizontal/
vertical coverage, plus independent gap paint with the main line disabled.

The solid-band comparison exposed padding-dependent affine sampling: dividing a
pixel coordinate by a source width and multiplying by the same width could round
an edge differently when the mask had more transparent padding. The sampler now
computes the scale ratio first. A general padding-invariance property guards the
fix, and the band comparison remains exact at three output resolutions.


Dashed underline/strike coverage integrates a periodic rectangular wave. Full
cycles and at most two partial cycles suffice for a pixel, so arbitrarily small
valid periods cannot cause an unbounded repetition loop. Phase follows visual
character order and continues across spaces, bidi runs and paint-only changes.
Each ink is quantized independently; equal main/gap paints use one silhouette.
Inactive glyph colors do not break kerning or split decoration opacity groups.

The twelve-page `dashed_decoration_proof` compares two dash arrays in every writing
mode against independently enumerated solid rectangles. It covers fractional
lengths, spot/process paints, tints, opacity, opposite overprint choices and affine
placement. The kernel compares plain and transformed output at 72/144/216 dpi.
Reference masks are rasterized locally before integer page placement: translating
floating-point geometry first would round away near-half-pixel coverage. Round and projecting caps are described below; straight fitting and path decoration
geometry are described below.


Round/projecting dash caps now extend the real geometric segment endpoints. A
first pass identifies uninterrupted geometry before the renderer paints each
character-owned portion. Main-ink caps beyond the segment have no invented gap
paint. Caps that overlap are unioned before opacity. Projecting caps integrate
exact intervals; round caps use bounded adaptive quadrature over circle sections.
Work is independent of the number of repeated dashes, including subnormal periods.

The 24-page `capped_decoration_proof` compares both cap shapes, two arrays/weights
and three writing modes against independently enumerated capsules. The reference
uses analytic disk/rectangle antiderivatives rather than the production quadrature.
Plate and PDF sample comparisons require exact equality, including fractional
weights, spot/process paints, tint, opacity, overprint and affine placement.
Kernel properties also compare projected interval unions and analytic circle,
capsule and overlapping-circle areas. Metrics and carets remain unchanged.

Automatic cap thickness also reaches cross-gutter contribution bounds through a
font-metric query that does not recompose the story. A separate property requires
visible ink and exact plate agreement on both sides of a spread for automatic
and explicit line weights at three resolutions.


Dotted underline/strike patterns use center-to-center point intervals and a diameter
set by line weight. Coverage shares the bounded round-cap integrator, representing
dots as zero-length segments without altering the native DotArray model. Duplicate
centers and overlapping dots union before opacity; gaps stay inside the line's
base rectangle. Changing weight leaves center positions unchanged.

The twelve-page `dotted_decoration_proof` compares independently enumerated circles
using analytic disk/rectangle antiderivatives. Cases cover two spacing arrays and
weights, all writing modes, spot/process inks, tints, opacity, opposite overprint
and affine placement. Plain and transformed plates agree exactly at 72/144/216 dpi;
Poppler verifies sample and rendered-page equality. All pages were visually
inspected. The cross-gutter property includes automatic and explicit dot weights
on both spread sides and requires real neighboring-page ink.


Straight decoration fitting resolves a complete visual segment before any
character-owned pieces are painted. It changes the requested component and keeps
fixed lengths intact. Repetition selection examines at most four cycle counts
per array prefix, independent of the number of repeats. Computation stays in f64
through periodic area integration, including subnormal input periods. A fitted
terminal marker prevents remainder rounding from dropping the final dot/cap.

The independent fitted-decoration reference exhaustively enumerates controlled
fixture sequences, then integrates analytic rectangles, capsules and circles.
It covers dash-only, gap-only and combined fitting, all cap shapes and writing
modes, dot spacing, independent inks and affine placement. This checks Schist's
specified algorithm, not equivalence with native InDesign output.

The fitted proof contains 24 pages. Plain and affine plate samples agree exactly
at 72/144/216 dpi; Poppler sample and page comparisons pass. All pages were visually
inspected, and final renders match the reviewed pages exactly. Its unequal-dot
case caught a terminal-circle loss from a rounded remainder, now covered by a
separate property over multiple spacing arrays and many segment lengths.


Text language now follows paragraph defaults and local character overrides into
both preview and print shaping. `language_proof` compares Turkic capitals,
Lithuanian combining-dot removal and Romanian localized glyphs with independently
authored Unicode text, including explicit language resets. Its 24 pages cover
horizontal and both vertical modes, spot/process glyph paints, tint, opacity,
overprint and affine frames. The kernel property compares plain and transformed
plate samples at 72, 144 and 216 dpi; `check-design-output` compares extracted PDF
samples and Poppler pages. Native application agreement, dictionary-based
hyphenation and spell checking remain outside this verification.

Path text uses the same composed line specification in preview and print. Baseline
coordinates, handles, start offset and interval scale with output resolution;
glyph outlines rotate individually before the frame affine. Frame paint retains
its actual path even for zero-height baselines. Page-contribution checks include
rotated glyph extents, and the neighboring-page property requires nonempty,
identical plates at 72/144/216 dpi on either spread side.

`text_path_proof` pairs Design output with independently constructed engine path
specifications for left/center/right bracket alignment, horizontal/opposing
vertical baseline directions and a cubic curve, including spot fill, process
stroke, tint, opacity, overprint and affine placement. It also covers solid,
striped, butt/round/projecting dashed and dotted underlines/strikes with gap ink.
This checks integration with the shared renderer, not independent native
application agreement. Additional native path effects remain open.

Path decoration masks retain their existing inline area integration and fitting.
Equal consecutive paint fragments are combined before bending, so character
boundaries cannot introduce resampling seams. A strip mesh follows the same
flattened baseline used by glyphs and carets; offset intersections use a miter
limited to four times the offset, then a bevel. Four-by-four destination samples
read the source coverage cells, taking their union before averaging and applying
opacity. Scratch coverage uses one row, independently of output height. Cardinal
rotations and integer translations preserve every source coverage value exactly.
This bounded raster policy does not establish native corner fitting or distortion
semantics. Cross-gutter bounds include automatic font metrics and join extents;
the neighboring-page property also requires actual ink from decorated spaces.

### Generated list proof

`make design-lists-proof` writes `/tmp/schist-lists-proof.pdf`, also included in
`make check-design-output`. Twenty-four automatic-list cases are paired with ordinary
text frames positioned independently from font metrics. They cover Unicode
bullets, the decimal transition from 9 to 10, left/center/right marker alignment,
12/24-point markers beside 18-point body text, strokes, spot tints, opacity and
overprint. Additional cases cover upper/lower Roman and alphabetic counters,
Z-to-AA transitions, three padding widths crossing digit boundaries, and a hidden
number whose tab still positions the body. The kernel comparison checks every plate at 72, 144 and 216 DPI; the
PDF check compares both extracted samples and Poppler-rendered pages. The 48
144-DPI proof pages were rendered without diagnostics and visually reviewed.
This verifies shared-renderer integration, not native InDesign output agreement.

Generated-marker bounds are also checked independently of the frame rectangle:
right-aligned markers contribute nonempty cyan ink on either neighboring page,
including a half-turn of the source frame. Every plate sample agrees with a
reference frame explicitly placed on that page at 72/144/216 DPI.


The multilevel list extension adds four generated/reference pairs: Roman parents
with alphabetic children, continuously numbered children, three nested formats,
and a repeated parent number that restarts its child. The independent reference
contains literal labels in ordinary text frames. Kernel plate comparisons pass
at 72/144/216 DPI, and all 48 list-proof pages were visually inspected with no
clipping or pairwise placement differences. The full multilevel sweep passes apart from the unchanged sandbox-denied
clipboard listener. These synthetic cases establish shared-renderer integration, not external native
application agreement.


### Paragraph tab proof

`make design-tabs-proof` writes `/tmp/schist-tabs-proof.pdf`, also included in
`make check-design-output`. Twelve tabbed cases are paired with ordinary text
frames at independently fixed positions. The cases cover horizontal and both
vertical writing modes, 12/18-point type, first-line indents, spot/process paint,
strokes, opacity and overprint. Kernel checks compare every plate at 72/144/216
DPI; the PDF check compares extracted samples and Poppler-rendered page pairs.

The comparison exposed position-dependent rounding in glyph outline coverage.
Ordinary glyph strokes now rasterize in local coordinates and apply their integer
translation to the finished mask, matching bitmap fill placement. A distant tab
therefore cannot change a glyph's edge coverage. A separate engine property checks
that invariant in all writing modes and at several sizes. Path glyphs retain their
continuous baseline transform.

All 24 tab-proof pages were rendered at 144 DPI without Poppler diagnostics and
visually inspected. Each tabbed/reference pair matches exactly, including both
vertical progressions. This establishes shared-renderer integration rather than
external native application agreement.

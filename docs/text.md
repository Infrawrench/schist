# Text layers

The Type tool has a single-row options bar for font family, style, size,
alignment and colour, followed by the Character panel button and text edit
cancel/commit buttons. Click a numeric value to type it; Up/Down adjusts it
and Shift takes larger steps. Enter finishes the field and Escape releases
it without cancelling the text edit.

Selecting an existing text layer loads its base fill into the foreground
swatch and preserves its character colors. To recolor it, select the text
first, then choose a new foreground color during the edit. Committing applies
that color to the whole layer; Undo restores its previous fills.

The **Character** tab opens in the right sidebar when you select Type. It
contains leading, tracking, and **Kerning** (AV), **Ligatures** (fi),
**Discretionary ligatures** (st), and **Small caps** (Tt). Hover the buttons
for their names. These are OpenType features
from the selected font; a font without a requested feature keeps its
ordinary glyphs. They apply to the whole text layer, including its style
runs. Font, style and size can still be changed for selected characters.

Existing text keeps its previous layout until an OpenType control is used.
New overrides use rustybuzz for glyph substitution and positioning. The
serialized `TextSpec.features` also accepts four-byte OpenType tags and
numeric values, including numbered stylistic sets and alternate glyphs.
The Character panel also has **Paragraph direction** and **Writing mode**.
Direction defaults to **Automatic**: each paragraph uses its first strong
Unicode character. Choose **Left to right** or **Right to left** to set the
base direction explicitly. Arabic and Hebrew runs shape in their reading
direction while Latin words and numbers keep theirs. Text stays in logical
Unicode order for typing, copying, deletion and style ranges. Use a font
with glyphs for the script; character selections can use different fonts.

**Vertical, columns to the left** and **Vertical, columns to the right** set
text from top to bottom and control where subsequent columns appear. CJK
glyphs stay upright, Latin runs rotate clockwise, and fonts with vertical
OpenType forms supply punctuation alternates. Punctuation that requires an
alternate falls back to its Unicode vertical orientation when the font has
none. Leading sets column spacing; a serialized `wrap_width` sets column
length. CJK wrapping uses Unicode line-break opportunities without requiring
spaces; unbreakable words retain the existing overflow behavior.

Arrow keys move in the displayed direction, mouse clicks and drags use the
same shaped positions as the glyphs, and selections may occupy separate
areas when a logical range crosses bidi runs. Backspace/Delete remove whole
Unicode graphemes, including Arabic/Hebrew combining marks and emoji
sequences. Directional run boundaries remember which side of the boundary
the caret occupies. Vertical text uses a horizontal insertion caret.

Changing writing mode or direction updates the current text edit and is
undoable when committed. Choosing a vertical mode switches off text on a
path; choosing **On path** switches back to horizontal writing. Existing
horizontal Latin text keeps its previous geometry until shaping is needed
or explicitly requested by its settings.

To set text on a curve:

1. Draw a path with Freeform Pen or Curvature Pen, or select a live shape
   with Path Selection. The ordinary Pen can create a live shape in its
   **Shape** mode.
2. Switch to Type and choose **On path** under **Text on a path** in the
   Character panel, before creating text or while editing an existing layer.
3. Use **Path offset** to move the text along the baseline. Alignment places
   it at the start, centre or end of the path.

Text uses a copy of the active path's first subpath with at least two
anchors. As with Path Selection, an active live shape takes precedence
over a separately stored path. Later edits to the source path do not change that copy; toggle
**Straight** and then **On path** to take a fresh copy. The baseline stays with
the text layer. Glyphs rotate along the curve, and the insertion caret and
mouse selection follow it. Extra lines keep their normal line spacing;
word wrapping is disabled on paths. Text beyond either end continues
along the endpoint's tangent. A degenerate path uses ordinary text layout.

Choose **Straight** to return to the layer's ordinary layout box.
Text, direction, writing modes, paths and feature settings participate in the existing text-edit
undo operation and survive PSD and PSB save/reopen in Schist's `PsTx`
layer block. Supported horizontal straight text also writes native `TySh`
type metadata, so other capable editors can edit its text and font runs.
Paths and unsupported typography retain their rendered pixels in other
editors. See [PSD interchange](psd-interchange.md) for the supported subset.

`make check-text` runs the layout, editing, persistence and Affinity import
regression tests, including bundled Noto Arabic, Hebrew and Japanese font
fixtures for bidi and vertical typography. `make check-text-directions` also
checks the editor integration and translation catalogs.

The engine uses [unicode-bidi](https://docs.rs/unicode-bidi/latest/unicode_bidi/)
for paragraph and line ordering, [rustybuzz](https://docs.rs/rustybuzz/latest/rustybuzz/)
for directional OpenType shaping, and [unicode-vo](https://docs.rs/unicode-vo/latest/unicode_vo/)
for Unicode vertical orientation.

# Text layers

The Type tool has a single-row options bar for font family, style, size,
alignment and colour, followed by the Character panel button and text edit
cancel/commit buttons. Click a numeric value to type it; Up/Down adjusts it
and Shift takes larger steps. Enter finishes the field and Escape releases
it without cancelling the text edit.

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
Paragraph layout remains horizontal and left-to-right; bidi paragraph
layout and vertical writing are separate remaining work.

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
Text, paths and feature settings participate in the existing text-edit
undo operation and survive PSD and PSB save/reopen in Schist's `PsTx`
layer block. Other editors see the rendered pixels, as with other Schist
text layers.

`make check-text` runs the layout, editing, persistence and Affinity import
regression tests.

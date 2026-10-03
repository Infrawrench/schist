# Frame animation

Schist animates a layered document the way Photoshop's frame animation
does: one stack of layers, and a timeline of frames that each record, for
every layer, whether it is visible, its opacity and a position offset. Pixels
are shared by all frames, so a frame costs a few bytes per layer.

## Making an animation

**Layer ▸ Animation** has the commands:

- **Create Frame Animation** starts a timeline whose one frame is the
  document as it stands.
- **Make Frames From Layers** replaces the frames with one per top-level
  layer, bottom to top, each showing only that layer (layers inside a group
  keep their own visibility, so a group shows as it is). The usual way to
  start from a sprite sheet of layers.
- **Flatten Frames Into Layers** renders every frame into a new layer on top
  of the stack, each visible only in its own frame.
- **New Frame** duplicates the selected frame; **Delete Frame** deletes it
  and selects the one before; **Reverse Frames**; **Previous/Next Frame**
  (wrapping); **Play/Pause Animation**; **Delete Animation** removes the
  timeline and keeps the layers as the selected frame shows them.

The commands are registered like every other command, so they also appear in
Spotlight, can be bound to keys, recorded in [actions](actions.md) and called
over [MCP](mcp.md) (`cmd_animation_*`). Every frame operation is one undoable
step in History.

## The Timeline panel

The panel appears in the dock when the document has a frame animation. Like
other panels it can be dragged to another position, resized, hidden and
saved in a [workspace](workspaces.md) (`timeline`).

- **Frames** show as thumbnails with their number and delay. Click to
  select; drag one onto another to move it there.
- **Transport**: first, previous, play/pause, next, last, plus New Frame and
  Delete Frame.
- **Delay** sets the selected frame's delay; **Apply to All Frames** copies
  it to every frame. **Loop** is Once, 2×, 3×, 5× or Forever.
- **Onion skin** shows up to three earlier frames tinted red and three later
  ones tinted blue over the canvas, at the chosen opacity (nearer frames
  stronger). Display-only, never exported or saved; off by default each
  session.
- **Layer offset in this frame** moves the active layer by whole pixels in
  the selected frame only, with Reset.
- **New layers visible in all frames** is Photoshop's option of the same
  name: off, a layer added while one frame is selected is hidden in the others.

### How editing relates to frames

The live layers *are* the selected frame. Toggling a layer's eye, changing
its opacity, or undoing either changes the selected frame, without any extra
step. Painting, filters and every other pixel edit change the shared pixels,
so they show in every frame that shows the layer. The Move tool moves pixels
for all frames too, keeping the selected frame's offset; use the panel's
offset controls to move a layer in one frame only.

A layer added while a frame is selected gets its state in the other frames
the next time another frame is selected, following the new-layers rule.

### Playback

Play runs from the selected frame on the frames' own delays (scheduled
against a clock, so a slow render shortens the next wait rather than
drifting), and stops after the loop count. It shows rendered frames over
the canvas and never changes the document, its history or its saved state.
Clicking the canvas or running any command stops it.

## Export

**File ▸ Export ▸ Animation…** writes the timeline as:

| Format | Notes |
| --- | --- |
| GIF | 256 colours per palette, **per frame** or one **global** palette. A frame (or, globally, the animation) with few enough colours keeps them exactly; otherwise NeuQuant reduces them, with optional Floyd–Steinberg dithering. Transparency is one index, so pixels under half alpha become transparent and the rest opaque. Delays are hundredths of a second; anything under 20 ms is written as 20 ms because browsers slow shorter delays to 100 ms. **Disposal**: Automatic restores to background when any frame has transparency (so earlier frames never show through) and otherwise keeps the previous frame; Keep, Restore to background and Restore to previous are available explicitly. |
| APNG | Lossless 8-bit RGBA frames with full alpha; delays in milliseconds. Each frame replaces the canvas (blend op Source). |
| WebP | Lossless animated WebP with alpha; delays in milliseconds. |

All frames cover the whole canvas. Turning off **Transparency** flattens
every frame onto white first. The loop count is written natively: GIF's
NETSCAPE2.0 block counts repeats after the first play (and is omitted for
Once), APNG and WebP count total plays, with 0 meaning forever.

The encoders are pure Rust and work in the browser build too: `gif` and
`png` (MIT/Apache-2.0), `color_quant` (MIT) and `image-webp`
(MIT/Apache-2.0), all already in the dependency graph through `image`.
`image-webp` encodes lossless still images only; Schist writes the animated
container (`VP8X`, `ANIM`, `ANMF`) itself from Google's published
[WebP container specification](https://developers.google.com/speed/webp/docs/riff_container).
There is no pure-Rust lossy VP8 encoder, so lossy animated WebP is not
offered.

Export renders each frame with the CPU/GPU compositor at full document size.
Large documents with many frames take correspondingly long.

## Saving

The timeline is stored in PSD and PSB files in Schist's private
document-level `ScAn` block (JSON: frames, delays, loop count, the
new-layers option and per-layer states). Layers are referred to by their
position in the layer tree with their names as a check; if another
application reorders or renames layers, states are matched by unique name and
otherwise dropped rather than applied to the wrong layer. The selected frame
is also the layers' ordinary visibility and opacity, so any PSD reader opens
the file showing it.

Photoshop's own frame animation data is preserved verbatim with every other
block Schist does not interpret, but it is not read, and it is not updated
when Schist's timeline changes. A file animated in Photoshop therefore opens
without a Schist timeline, and a file animated in both may disagree.

Other formats (`.af`, `.pdn`, `.xcf`, flat images) do not carry the timeline.
Collaborative Schist Cloud sessions do not sync it yet.

## Limits

- Painting on a layer whose offset in the selected frame is not zero paints
  in the layer's own coordinates; the stroke appears shifted by the offset.
  Mask refinement refuses such a layer.
- Playback and onion skins are drawn unrotated, so they are hidden while the
  view is rotated, and they are rendered at no more than 2048 px on the
  longer side, so they can look soft when zoomed in past that.
- Onion skins and frame thumbnails re-render after edits that change the
  frames they show; on large documents that is one composite per image.
- Frame offsets are whole pixels; there is no tweening, per-frame layer
  style, or video timeline.
- Animated GIF, APNG and WebP files open as their first frame, not as a
  timeline.

## Verification

`cargo test -p schist-core animation`, `-p schist-codec-psd --test animation`
(PSD and PSB round trips, unmatched layers, verbatim preservation),
`-p schist-commands-core animation` and `-p schist-animation` (encode then
decode with the `gif`, `png` and `image-webp` decoders: frame count, delays,
loop count, transparency, lossless pixels). When `ffprobe` and ImageMagick's
`identify` are installed, the same test checks frame count and size with
ffprobe (GIF, APNG) and delays with ImageMagick (GIF, WebP); set
`SCHIST_ANIMATION_ARTIFACT_DIR` to keep the files.

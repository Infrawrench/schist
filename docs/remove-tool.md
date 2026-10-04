# Remove tool

The Remove tool is in the healing group. Press `J` for the group and
Shift+`J` to cycle to it, or find it in Spotlight. Paint over an unwanted
object in one stroke. When you release, the object is filled in from its
surroundings. Brush size is the shared brush size, so `[` and `]` change
it, and pen pressure scales it (down to 10%). The stroke shows in
translucent pink until its result lands.

Options bar:

- **Size**: the brush size.
- **Sample All Layers**: reads the composite of all visible layers rather
  than the active layer alone. The result goes to the active pixel layer,
  which can be an empty layer created for retouching. If the active layer
  is not a pixel layer (a group, text, an adjustment, or a locked layer),
  a new empty layer named "Removed" is created above it. Without this
  option, a removal needs an unlocked pixel layer.
- **Remove After Each Stroke**: on by default. Turn it off to paint
  several strokes, then press Enter or click **Apply** to remove them all
  in one fill. Escape discards waiting strokes, and switching tools
  applies them.

An active selection limits what a removal writes. Each removal, including
a layer it creates, is one undo step.

## What happens on release

The code is in `plugins/tools-retouch/src/remove.rs`.

1. **Mask.** Each pixel whose centre falls inside the stroke is painted.
   The stroke is drawn as discs joined by tapered capsules, so its width
   follows pen pressure. The painted area is then grown by 10% of the
   brush radius (1 to 12 px) to catch the object's soft edge, and
   feathered over another 8% (1 to 6 px). Distances come from an exact
   Euclidean distance transform.
2. **Context window.** The window is the hole plus half its longer side on
   every side (24 to 768 px). That is the proportion the inpainting
   network was trained on, and the one Content-Aware Fill uses. At an
   image edge the window slides inwards rather than shrinking.
3. **Fill.** The fill method depends on the hole's thickness, meaning the
   greatest distance from any hole pixel to a kept pixel:
   - Up to 5.5 px, spot healing's ring interpolation is used, generalised
     to any hole shape.
   - Thicker holes go through Content-Aware Fill's pipeline: the built-in
     `inpaint.onnx` network for layout, then exemplar patch synthesis for
     texture, then seam relaxation for tone.

   The network runs on the GPU when the effects backend can run it (see
   [neural-gpu.md](neural-gpu.md)). Otherwise it runs in tract on the CPU.
   The threshold was measured with
   `cargo run --release -p schist-tools-retouch --example remove_eval`.
   Below about 5.5 px, patch synthesis adds more edges than the photo had
   and has roughly 10% more error. Above it, the fill keeps the texture
   that interpolation smears. The table is in the doc comment on
   `SPOT_HEAL_MAX_THICKNESS`.
4. **Blend.** The fill is mixed into the target layer with the feathered
   weights. The mix uses premultiplied colour, so a fill on an empty
   layer brings no dark fringe.

If the network is not available, the same pipeline runs without it:
diffusion seeds the hole and then patch synthesis runs. Natively the
network is built in, so this happens only if it fails to load. In the
browser it happens until the model has been fetched. When it happens,
the status bar says so and offers **Manage Models**.

## Off the UI thread

Natively, a removal is a `BackgroundEdit`, defined in
`crates/plugin-api/src/background.rs`. The editor runs it from
`crates/editor/src/workspace/tool_jobs.rs` in three stages:

- It prepares the edit on the UI thread, which copies the window's pixels.
- It fills on a worker thread.
- It applies the result on the UI thread as one history entry.

Painting continues while a removal runs. New strokes queue behind it and
are prepared from the document the previous removal left. The status bar
shows the name, percentage and progress bar, with **Cancel**. Escape
also cancels.

If the document changed while the fill ran (an undo, another tool), the
result is discarded and the edit is prepared again from the current
document, up to three times. After that, the result is written anyway.
If the document was switched or closed in the meantime, nothing is
written.

## Limits

- Quality is Content-Aware Fill's quality. The network is small and sees
  the window at 160 px. On large holes over structured backgrounds
  (fences, buildings, tree lines) the result can be patchy or blocky
  rather than convincing. No new model was added for this tool.
- Thin strokes over strong texture, such as a wire across rock, are
  interpolated and can leave a visible smooth band. A wider brush sends
  them to the fill instead.
- Cancelling takes effect between stages and every 32 patch placements.
  The network's forward pass itself cannot be interrupted.
- In the browser, the removal runs inline on the main thread, with no
  progress bar, because the web build has no worker thread to give it.
- Removals are not recorded in actions, because no retouch or paint
  stroke is recordable.
- The MCP server drives the tool synchronously. Only the default
  per-stroke mode is useful there.

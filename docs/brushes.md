# Brush presets and stroke controls

Brush, Pencil and Eraser share a **Preset** button in the options bar. It opens
the brush controls and your named preset list. Size, hardness and opacity remain
in the options bar.

* **Texture** chooses the round footprint (**None**), a procedural grain tip,
  or separated bristles (**Bristle Detail**). Both textured tips scale with the
  brush and retain the hardness control. They do not require external files.
* **Spacing** is the distance between dabs, as a percentage of the full brush
  diameter. Larger values leave separated stamps.
* **Spatter** scatters dabs in a disk around the stroke. Its percentage is the
  maximum offset relative to the full diameter; zero keeps dabs on the path.
* **Smoothing** controls a distance-based stabilizer, in document pixels. Zero
  disables it. Larger values suppress more jitter and make the painted line
  follow farther behind the pointer. Releasing finishes the pending tail at the
  release position.
* **Pressure** changes the exponent (γ) of tablet pressure's size response.
  **1** is linear, below 1 makes light pressure larger, and above 1 requires
  more pressure for the same size. A mouse uses full pressure, so this setting
  has no effect on mouse strokes. Opacity is independent of this curve.

Type a name and choose **Save** to store all seven brush parameters and the tip:
size, hardness, opacity, spacing, spatter, smoothing and pressure response.
Selecting a preset immediately restores them. **Update** replaces the recipe
with the same name. Change the name and **Save** to keep a separate variation;
**Delete** removes the named preset. Names are limited to 64 characters and the
library holds up to 128 presets. A preset stores brush settings, not foreground
colour or document content.

Presets survive restarts in `brush-presets.json` in Schist's configuration
folder on native builds and in browser local storage on the web. They are local
to that profile and are not embedded in documents or synchronized between
devices. Deleting site data clears browser presets. Damaged preset files fall
back to an empty library; saved numeric values are validated before use.

Stroke settings are captured when the pointer goes down. Changing a preset
affects subsequent strokes. Dabs use interpolated pressure, and deterministic
scattering and spatial sampling keep results independent of pointer-event
batching along the same path. Within a stroke, overlapping dabs retain the
tool's opacity ceiling. A completed stroke is one undo step; cancelling restores
the layer. Selection coverage still clips every dab. Retouch tools retain their
existing round footprints.

The first texture choices are procedural. Importing bitmap brush tips or brush
packs, pressure-controlled opacity, and tilt-driven brush rotation are not
included.

Run `make test-brush-workflows` for engine, preset and retouch regressions, and
`make check-brush-workflows` to check the editor integration.

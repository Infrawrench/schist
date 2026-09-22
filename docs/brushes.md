# Brush presets and stroke controls

Brush, Pencil and Eraser share a **Preset** button in the options bar. It opens
the brush controls and your named preset list. Size, hardness and opacity remain
in the options bar.

* **Texture** chooses the round footprint (**None**), a procedural grain tip,
  separated bristles (**Bristle Detail**), or an imported **Bitmap**. Procedural
  textures retain the hardness control. Bitmap masks keep their own edges and
  aspect ratio; their longest edge maps to the brush size.
* **Spacing** is the distance between dabs, as a percentage of the full brush
  diameter. Larger values leave separated stamps.
* **Spatter** scatters dabs in a disk around the stroke. Its percentage is the
  maximum offset relative to the full diameter; zero keeps dabs on the path.
* **Smoothing** controls a distance-based stabilizer, in document pixels. Zero
  disables it. Larger values suppress more jitter and make the painted line
  follow farther behind the pointer. Releasing finishes the pending tail at the
  release position.
* **Pressure → Size** changes the exponent (γ) of tablet pressure's size response.
  **1** is linear, below 1 makes light pressure larger, and above 1 requires
  more pressure for the same size. A mouse uses full pressure, so this setting
  has no effect on mouse strokes.
* **Pressure → Opacity** enables a separate linear pressure response for opacity.
  Light pressure deposits less ink. Each pixel retains the strongest coverage
  reached in that stroke, up to the options-bar opacity ceiling. The size curve
  does not change the opacity response.
* **Rotation → Angle** rotates bitmap and procedural tips clockwise, from −180°
  to 180°. **Pen tilt** adds the stylus tilt direction to this angle in the
  browser and on supported desktop tablet backends. View rotation is removed
  so the tip follows the physical pen direction even on a rotated canvas.
  A vertical pen retains its previous orientation during a stroke. Mouse input
  and missing/invalid orientation use the manual angle; a previous tablet's
  sample is never reused for a mouse stroke.
  The control shows **Not available** when the latest canvas sample has no
  orientation. It can still be enabled in advance on a supported desktop host;
  that setting is saved in brush presets. iOS/Android keep the control disabled.

Native input comes from GPUI's AppKit tablet-point events, Windows pen pointer
messages, Wayland tablet-v2 frames, and X11 named absolute tilt valuators with
known degree units (as supplied by xf86-input-wacom). Windows and Linux deliver
orientation-only updates even if the pointer does not move. Legacy Wintab-only
Windows drivers, X11 axes with unknown units, and native mobile orientation are
not supported. Browser input continues to use real Pointer Events `tiltX` and
`tiltY`. A circular brush may not visibly change: choose an asymmetric bitmap
or a narrow procedural tip to see rotation.

These backends have conversion and routing regression coverage, but physical
macOS/Windows/Linux tablets were unavailable for verification. Before relying
on a tablet, check the tip direction with the canvas upright and rotated, then
switch to a mouse and confirm the manual angle is restored. Platform sources,
axis conventions and GPUI validation are documented in the dependency's
[pen orientation notes](https://github.com/IAmJSD/gpui/blob/3654a9bb3e5aa6d85c67877ffe0567be1a704c1d/docs/pen-tilt.md).

Type a name and choose **Save** to store the brush parameters, dynamics and
embedded bitmap mask, including pressure opacity, angle and pen tilt.
Selecting a preset immediately restores them. **Update** replaces the recipe
with the same name. Change the name and **Save** to keep a separate variation;
**Delete** removes the named preset. Names are limited to 64 characters and the
library holds up to 128 presets. A preset stores brush settings, not foreground
colour or document content.

Presets survive restarts in `brush-presets.json` in Schist's configuration
folder on native builds and in browser local storage on the web. They are local
to that profile and are not embedded in documents or synchronized between
devices. Deleting site data clears browser presets. Damaged preset files fall
back to an empty library; saved numeric values and mask dimensions are validated
before use. Individual tips are limited to 1024 × 1024 pixels. The library allows
4 MiB of mask samples in total and import/storage JSON is capped at 20 MiB.
Browser storage quotas may be smaller: failed saves leave the current library
unchanged and show a failure status.

Stroke settings are captured when the pointer goes down. Changing a preset
affects subsequent strokes. Dabs use interpolated pressure and rotation, and deterministic
scattering and spatial sampling keep results independent of pointer-event
batching along the same path. Within a stroke, overlapping dabs retain the
tool's opacity ceiling. A completed stroke is one undo step; cancelling restores
the layer. Selection coverage still clips every dab. Retouch tools retain their
existing round footprints.

## Import and export

In the Preset panel, **Import** accepts:

- PNG, JPEG, WebP and TIFF images. If any pixel has transparency, its alpha
  becomes the mask. For fully opaque images, black paints and white is
  transparent, with intermediate luminance producing intermediate coverage.
  Oversized images are rejected before allocation; resize them before importing.
- GIMP `.gbr` version 2 brushes: grayscale masks and RGBA alpha masks, including
  names and spacing. Colour is supplied by Schist's foreground colour.
- Photoshop `.abr` sampled masks: versions 1 and 2, and versions 6 and 10 with
  subversion 1 or 2, using 8-bit raw or row-wise PackBits compression. Version 2
  names and spacing are retained. Modern packs use numbered names and 25%
  spacing because their dynamics descriptors are not interpreted. Their sampled
  masks are imported, not Photoshop's brush engine or its proprietary dynamics.
  Unsupported computed brushes, bit depths and versions produce an unsupported
  format error; corrupt files fail as a whole without partially adding presets.
- Schist `.schist-brushes` packs, described below, preserving every setting and
  embedded tip. **Export** writes all saved presets in this portable format.

Import saves the whole resulting library before selecting the first new preset.
A duplicate name receives a numbered suffix; existing presets are never replaced.
Failures leave both the active recipe and library unchanged. Imports are local,
with no external files needed after the masks have been saved.

## Portable pack format

A UTF-8 JSON file with extension `.schist-brushes`, `version: 1`, and a `presets`
array. Each recipe uses the existing preset fields, with optional `bitmap` and
new dynamics fields. Old recipes without the new fields retain their behavior.
Mask `pixels` are row-major coverage bytes: zero is transparent and 255 is full
coverage. For example:

```json
{
  "version": 1,
  "presets": [{
    "name": "Three-pixel tip",
    "size": 30,
    "hardness": 1,
    "opacity": 0.8,
    "dynamics": {
      "tip": "Bitmap",
      "spacing": 0.25,
      "scatter": 0,
      "stabilization": 0,
      "pressure_gamma": 1,
      "pressure_opacity": true,
      "rotation": 0,
      "tilt_rotation": false
    },
    "bitmap": { "width": 3, "height": 1, "pixels": [128, 255, 128] }
  }]
}
```

## Implementation references and verification

The ABR reader was independently written against the byte layouts in
[GIMP's public brush reader](https://github.com/GNOME/gimp/blob/master/app/core/gimpbrush-load.c).
No Adobe SDK headers were read, and no Photoshop plug-in support is involved.
The [GIMP brush format specification](https://developer.gimp.org/core/standards/gbr/)
defines GBR, and [W3C Pointer Events](https://www.w3.org/TR/pointerevents/)
defines pen tilt. No third-party source code or brush artwork is embedded.

Run `make test-richer-brushes` for engine, import, preset and retouch regressions,
`make check-brush-workflows` for native integration, and
`make check-richer-brushes-web` for the browser input path. `make check-i18n`
checks translated labels and font coverage. Tests cover synthetic ABR samples,
truncation and corrupt runs; they do not certify every third-party ABR exporter.
Physical tablet/browser input still requires a device smoke test.

The **Symmetry** button beside **Preset** adds mirror and radial painting,
movable axes, and a seamless tile preview with wraparound painting. See
[symmetry and seamless painting](symmetry-painting.md) for the workflow.

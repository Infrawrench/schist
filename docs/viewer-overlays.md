# Viewer overlays and colour-vision proofing

Three display aids. None of them changes document pixels, saved files,
exports, prints or thumbnails: they exist only on screen.

| Aid | Where | Menu | Default key |
| --- | --- | --- | --- |
| Colour-vision simulation | Editor canvas | View ▸ Color Vision | none |
| Clipping warnings | Canvas, gallery viewer, Compare, Similar photos review | View ▸ Clipping Warnings | ⌥J (Alt+J) |
| Focus peaking | Same as clipping | View ▸ Focus Peaking | ⌥⇧J (Alt+Shift+J) |

Lightroom toggles clipping with a bare J, but Schist follows Photoshop's
tool letters, where J is the spot healing brush and Shift+J cycles its
group. The overlay keys therefore carry Alt, so they cannot collide with a
tool letter, and the gallery's culling keys ignore Alt chords. Every item
is listed by Spotlight (⌘⇧P) through the menu model and can be rebound in
`keymap.json` as `view:clipping`, `view:focus_peaking`, `view:vision.normal`
or `view:vision.<kind>`; see the [README](../README.md#mouse-and-touchpad).

The overlay switches last for the session: a warning layer left on from a
previous launch would look like damage to the photo. Peaking colour and
sensitivity, and the anomaly severity, are kept with the other view
preferences.

## Colour-vision simulation

View ▸ Color Vision offers Normal Vision, protanopia, deuteranopia and
tritanopia (missing L, M or S cones), protanomaly, deuteranomaly and
tritanomaly (shifted cones), and achromatopsia. The three anomalies use the
Mild, Moderate or Strong severity in the same menu: 0.3, 0.6 or 0.9 on the
scale where 1.0 is the matching dichromacy.

The model is Machado, Oliveira and Fernandes, "A Physiologically-based Model
for Simulation of Color Vision Deficiency" (IEEE TVCG, 2009). Their published
matrices are tabulated at severities 0.0 to 1.0 in steps of 0.1;
intermediate severities interpolate linearly between neighbours.
Achromatopsia replaces each colour with its Rec. 709 relative luminance. Both
operate on linear light: pixels are decoded with the sRGB curve, multiplied
and re-encoded.

The simulation is the last step of the display path, after soft proofing
(Proof Colors) and the display transform, because it models the viewer
rather than a device. It is part of the tile-cached display conversion
(`schist_colormgmt::VisionSimulation`), so changing it rebuilds the cached
display tiles once, exactly like changing the proof. The kernel is the
matrix/TRC ICC shader with the simulation matrix between sRGB decode and
encode tables. It runs on the compute backend when one is installed and the
work is large enough; otherwise the CPU evaluates the same tables. In the
browser it joins the GPU sequence after the proof and display transforms.

**Limits.** The matrices assume sRGB primaries. On a wide-gamut display the
values entering the simulation are display-encoded, so the result is
approximate. The model represents typical observers; individual vision
varies. Gallery viewers do not apply the simulation.

## Clipping warnings

The warning is computed per channel on the displayed image:

* every channel at 255: a blown highlight, painted solid red;
* some channels at 255, such as a saturated sky or flower: a half-strength red
  wash;
* every channel at 0: crushed shadow, painted solid blue;
* some channels at 0: a half-strength blue wash.

Because the test is made on the displayed frame, it reflects the active
display transform, soft proof and colour-vision simulation. A small specular
highlight can average below 255 when zoomed out. Zoom in to inspect isolated
pixels.

## Focus peaking

Peaking marks edge energy: the Sobel gradient of the displayed image's luma.
**Low**, **Medium** and **High** sensitivity trigger on luma steps of about
56, 36 and 22 grey levels. Choose a colour from red, yellow (the default),
green, cyan, magenta or white under **Focus Peaking Options**. Selecting a
colour or sensitivity also turns peaking on. Peaking draws over the clipping
warnings.

## Cost and caching

Both overlays run on the displayed image, never the full-resolution
document:

* **Canvas.** The overlays are painted into the resampled viewport frame,
  which is at most the window's size. The overlay settings are part of the
  frame's cache key, so a cached frame is reused until the view, the
  document or the settings change. Only pixels within the canvas are tested,
  so the surround and canvas edge do not trigger peaking. Transparent areas
  show the checkerboard, whose squares can register as edges at High
  sensitivity.
* **Gallery.** The viewer, Compare panes and the Similar photos review draw a
  transparent overlay layer above each photo. Each layer is computed from a
  box-filtered copy of the decoded photo at about twice its displayed size,
  rounded up to a 512-pixel step and capped at 4096 pixels. It is cached for
  that image and those settings, with up to six layers retained. Zooming
  recomputes the layer only after crossing a 512-pixel step. Very deep
  Compare zooms stretch the 4096-pixel layer rather than computing more
  detail.

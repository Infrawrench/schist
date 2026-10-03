# Camera Raw local adjustments

Camera Raw development of a RAW-backed layer can adjust part of the picture as
well as all of it. Open **Filter ▸ Camera Raw Development…** on a layer that
still holds its original capture; the **Masks** section sits under the global
sliders.

## Masks

**New mask** starts a mask from one of six shapes:

| Shape | What it covers |
| --- | --- |
| Brush | Strokes painted on the canvas with Size (a percentage of the image's longer side), Feather and Flow. Overlapping low-flow dabs build up. **Erase** takes coverage away. |
| Linear Gradient | Full effect up to the first line, fading to none at the second. Drag either line's handle, the middle handle to move both, or drag on the canvas to draw a new one. |
| Radial Gradient | Full effect inside the inner ellipse, fading to none at the outer one. Drag the centre, the major-axis handle (radius and rotation together) or the minor-axis handle; Feather sets the width of the fade. Dragging on the canvas draws a new one. |
| Select Subject | The salient subject, found by the Object Selection model (U^2-Netp). |
| Select Sky | The sky, found without a dedicated sky model; see the limits below. |
| Select Background | Everything Select Subject does not cover. |

**Add to mask** adds another shape to the selected mask. Each component after
the first combines with the coverage above it by **Add** (the larger of the two),
**Subtract** or **Intersect**, and any component can be inverted. **Invert
mask** inverts the combined result. **Show mask overlay** tints the selected
mask red in the preview; it is never part of the applied result.

Each mask has its own Temperature, Tint, Exposure (−4 to +4 EV), Contrast,
Highlights, Shadows, Clarity, Dehaze, Saturation and Sharpness. Negative
Sharpness softens. These are the global Camera Raw pipeline's own operations,
run over the picture and blended into it by the mask's coverage, in mask order,
after the global development. Up to 32 masks of up to 16 components each are
kept.

## Preview, apply and GPU use

Preview renders from the fast demosaic like the global sliders do. The globally
developed picture is kept between edits, so changing a mask re-runs only the
masks rather than decoding the sensor data again; a brush stroke renders when
the button is released. **OK** develops the capture at best quality with every
mask and records one undoable edit.

A mask's adjustments run as one GPU graph where the global Camera Raw filter
does (the resident compute path natively, the browser's WebGPU context on the
web), with the CPU path used when the GPU declines. The CPU path only processes
the bounding box of the mask's coverage, plus the reach of the widest blur
used.

## Saving and actions

Masks are stored with the development in Schist's private `ScRm` block beside
the `ScRw` capture block, so they survive PSD/PSB save and reopen, gallery
sidecars and saved versions (which are PSDs), and undo/redo. Detected masks are
saved as coverage rasters, so reopening never runs a model again. Other PSD
readers ignore the block and show the rendered pixels. Schist versions without
masks keep the block verbatim and render the development without it.

A recorded Camera Raw development records its masks too. A detected component
is recorded as the request — subject, sky or background — not as the coverage,
and is detected again on each layer the action replays on, as Lightroom does
when it syncs AI masks. Replay fails transactionally when that needs a model
that is not installed. Actions recorded before masks existed leave a layer's
masks unchanged.

## Limits

- Select Subject and Select Background need the Object Selection model from
  **Filter ▸ Neural Filters ▸ Manage Models**. It finds the most salient
  object; it does not distinguish people, separate several subjects, or offer
  Select People / Objects.
- Schist ships no sky segmentation model. Select Sky floods from the top edge
  through bright, smooth, blue or overcast-grey pixels and, when the depth
  model from Manage Models is installed, excludes near pixels. Refine Mask's
  colour-guided edge estimate then settles the edge. Skies that do not touch
  the top edge, heavily textured clouds, sunsets and low-contrast horizons can
  be missed or leak; combine with a brush or gradient to correct them.
- Detections run on at most 1024 pixels on the longer side (512 for the sky)
  and are kept as they were found. They are not refreshed when the global
  sliders change; delete the component and add it again to detect afresh.
- Local adjustments are applied to the display-referred development after the
  global controls, not in scene-linear light inside the sensor pipeline, so a
  strong local exposure behaves like the global Camera Raw filter's Exposure
  rather than like the RAW-domain exposure.
- There are no range masks (luminance, colour, depth), no brush auto-mask or
  density, no local Texture, Whites/Blacks, Noise, Moiré or Defringe, and no
  mask names or per-mask amount.
- Masks belong to RAW redevelopment only. Camera Raw used as an ordinary pixel
  filter or a filter-stack entry has no Masks section.
- Action libraries containing masks cannot be loaded by Schist versions that
  predate masks.

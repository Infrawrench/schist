# On-canvas filter controls

Open a supported filter from the Filter menu or edit an entry in a layer's
filter stack. Its dialog sits on the right, leaving the canvas available.
Drag the blue handles; the corresponding numeric values update immediately.
With Preview enabled, each movement renders from the original pixels. With
Preview disabled, handles and numbers still update and OK applies those values.
Cancel or Escape restores the original; OK records one undoable change.

| Filter | Handles and guides |
| --- | --- |
| Iris Blur | Center, elliptical outer radius, roundness, inward feather boundary |
| Spin Blur | Center, circular outer radius, inward feather boundary |
| Field Blur | Sharp line position, direction, transition boundary |
| Tilt-Shift | Band position, direction, sharp-band width, transition boundary |
| Radial Blur | Blur center |
| Lens Flare | Flare center |
| Lighting Effects, Spot/Point | Light position and falloff radius |
| Lighting Effects, Infinite | Light direction |
| Path Blur | Blur direction |

After selecting a handle, arrow keys move it one pixel in the filter's coordinate
space; Shift moves ten. Numeric sliders remain available for all
parameters, including blur strength. Scroll gestures over
the canvas pan or zoom using the normal view preferences. Handles follow view
rotation and zoom, selection bounds and smart-object placement. A drag continues
across the dialog and outside the canvas until the primary button is released.
Clicking the backdrop never invokes the painting tool.

Boundary guides show the filter's actual formulas: Iris uses the shorter image
side for radius and interpolates roundness toward the image aspect ratio; Spin
uses the longer side; light spread uses the diagonal. Field Blur and Tilt-Shift
use projected image extent, including the existing angle/position semantics.
Radius or transition boundaries can extend outside the image; use the numeric
controls or pan/zoom when a handle lies outside the visible canvas. Field Blur
remains one linear blur field, and Path Blur retains its direction/curve model.

## Plugin contract

`FilterPlugin::canvas_controls(values)` returns toolkit-independent
`FilterCanvasControl` descriptors. The default is an empty list, preserving
existing plugins. Metadata belongs to the plugin rather than filter-ID checks
in the editor. Conditional metadata (for example Infinite lighting) only exposes
parameters used by that mode. `geometry` supplies guides and named handles in
filter-buffer coordinates, and `move_handle` writes finite, range-clamped values
into the same `FilterValues` as numeric controls. Hosts translate to the filter
region, apply any source-to-layer placement, and then apply the viewport transform.

Run `make check-filter-canvas` for geometry, metadata/rendered-output equivalence,
and placed-source coordinate regression tests.

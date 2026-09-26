# Blends, masks, curves and 3D models

These tools cover the blend, unified masking, curve inspection and 3D workflows
described in [Affinity's September 2026 feature article](https://www.affinity.studio/blog/blend-tool-mask-tool-3d-model-support).
Background removal and person/object detection are separate features.

## Editable vector blends

Choose **Blend Tool** from the shape-tool group (or search for it in Spotlight).
Click two visible shape layers. The new layer holds both endpoint shapes and
their intermediate shapes; the original layers are hidden. Undo restores them.

- **Steps** sets the number of intermediate shapes. **Bias** and **Easing**
  control their distribution and morph progression.
- In **Shapes** mode, drag an endpoint node. Shift-drag moves that whole endpoint
  shape; Alt-drag sets a pair of smooth handles.
- **Draw spine** draws the path that positions the shapes. Spacing uses measured
  arc length. **Follow curve** turns shapes with the tangent.
- **Draw second rail** sets the opposite side of the blend, controlling its
  width and direction. Drag existing rail nodes to refine the result;
  Shift-drag moves a rail, and Alt-drag changes its handles.
- **Map points** connects a start node to an end node with two clicks.
  Automatic correspondence aligns contour winding and starting nodes; different
  point counts are matched by subdividing existing curves. Manual mapping
  swaps correspondences without dropping any points.
- Delete removes the current spine/second rail or resets manual mapping,
  depending on the selected mode. Escape cancels the current drag.

Blends interpolate fill, opacity, stroke colour and width. Up to 1,024
intermediate shapes and 4,096 anchors per input path are supported.

## Unified masking

Choose **Mask Tool** in the brush group with a layer selected. A mask is created
on the first edit. Brush mode shares the editor's brush size, hardness, opacity,
textured tips and dynamics. **Reveal** switches between revealing and concealing;
Alt temporarily reverses it. Rectangle and ellipse modes add sharp mask edges.

Linear and radial modes add live gradients. Their handles remain editable after
painting: the painted mask is retained separately and multiplied by the
gradients. Drag either endpoint, reverse the selected gradient, or delete it.
Up to 32 live gradients can be combined. Brush and shape edits respect the
selection; gradients cover the canvas.

**Sky (color and edges)** finds blue sky and bright clouds connected to the top
of the image, using colour confidence and edge boundaries. Tolerance and Reverse
control that result. This is a local colour/edge heuristic, so sunsets, enclosed
sky patches and buildings with similar colours may need brush refinement. It
does not run person detection, object detection or background removal.

Editing the mask with another destructive mask operation bakes its current
appearance. Undo restores the live gradients and their painted base.

## Curvature inspection and G2 snapping

The **Direct Selection** tool has **Curvature comb**, **Comb scale** and
**Snap G2 curvature** options. The comb shows signed curvature along each cubic
segment. When a smooth handle is dragged, G2 snapping solves the opposite
handle's direction and length to match curvature at the shared anchor. Anchors
and the dragged handle stay fixed. Degenerate joins or joins without a valid
solution retain normal handle editing.

## 3D layers

Open or place **GLB**, **OBJ** or **STL** files. Choose **3D Model Tool** to rotate,
move, scale or light the selected model by dragging. Its controls also provide
XYZ rotation, perspective projection, light direction/elevation/intensity and
ambient light. The mesh stays in the document and is rendered again when edited.

The CPU renderer supports depth testing, smooth normals, vertex colours,
embedded base-colour textures and directional/ambient lighting. GLB node
transforms are applied on import. OBJ supports polygon triangulation and
negative indices; STL supports ASCII and binary files. Import limits are
128 MB per file, one million vertices/triangles and 64 MB of decoded textures.
Rendered model bounds must fit within 16 megapixels.

GLB geometry and textures must be embedded. External OBJ materials, animation,
skinning and advanced PBR material channels are not currently supported. This
renderer is intended for still design compositions, rather than scene editing.
Translucent surfaces use the nearest visible surface; layered transparency and
refraction are not simulated.

### Image to 3D on desktop

Select an image layer and choose **Image → Image to 3D**. The first use offers
**Install local model**, which downloads the pinned
[TripoSR source and weights](https://github.com/VAST-AI-Research/TripoSR) and an
isolated Python environment into Schist's model directory. Python 3.10 or later
must already be installed. `SCHIST_3D_PYTHON` can select its executable.
Allow several gigabytes of disk space. CPU machines use PyTorch's CPU packages;
systems with NVIDIA tooling install the default CUDA-capable package.

**Reconstruct** uses the selected layer, its mask and the active selection to
generate a full mesh locally. Existing transparency is composited over neutral
grey. No segmentation or background removal is run. For best results, supply
an isolated object with useful transparency. Unseen surfaces are inferred, so
their shape and appearance may differ from the real object.

After installation, inference works offline using CUDA when available and the
CPU otherwise. CPU reconstruction can take minutes. The model uses a 192³
extraction grid and CPU marching cubes, without compiling a CUDA extension.
Cancel terminates the inference or installation process tree. The result is
inserted into its original document, even if another tab becomes active.

The browser supports imported and saved 3D models; Python reconstruction is a
desktop workflow. The standalone helper also supports 128³ and 256³ grids:

```sh
python3 tools/image-to-3d.py --root /path/to/triposr install
/path/to/triposr/venv/bin/python tools/image-to-3d.py \
  --root /path/to/triposr run --input object.png --output object.glb --resolution 192
```

## Saving and verification

PSD/PSB saves retain blend recipes (`scBl`), live masks (`scMk`) and compressed
3D sources (`sc3D`) with portable raster previews. Other editors can display
the previews but may discard Schist's private editing data when saving.
Rasterize, direct pixel painting or applying a raster filter stack bakes a
blend/model; undo restores it. Filter stacks continue to retain their own
editable filter parameters and raster input.
Saves during a drag use the committed layer, excluding temporary previews.

```sh
make test-creative-tools
make lint-creative-tools
make check-app
make check-app-web
make check-i18n
make render-model3d PROFILE=debug ARGS="object.glb preview.png 30"
```

The tests cover numerical arc spacing and G2 continuity; blend mappings, rails,
cancel and history; painted/live masks; model imports, lighting and transforms;
and retained sources across PSD saves. New strings are present in every existing
locale catalog. Catalog audits check keys and syntax; specialized technical
translations, especially in smaller language catalogs, still benefit from
native-speaker review.

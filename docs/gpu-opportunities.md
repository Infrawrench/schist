# GPU coverage and remaining opportunities

The compute backend covers the remaining image-processing stages from the
original map. Native callers use `schist-fx`; browser callers explicitly await
WebGPU. Unsupported graphs/profiles, device failures, small native workloads,
and jobs exceeding allocation limits retain their CPU implementations.

## Coverage

| Area | Implemented work | Browser entry point / boundary |
| --- | --- | --- |
| Adjustments | All direct adjustments, plus exact Auto Tone/Contrast/Color percentile selection and clipped color statistics. Histogram reductions retain HDR values and hidden RGB. | Destructive preview/Apply and automatic corrections use the captured filter queue. |
| Layer effects | Complete style assembly: content blur, fill opacity, shadows, glows, satin, bevel shading, color/gradient overlays and strokes, sharing compositor blend formulas. | Native style preparation uses the resident graph. Browser synchronous style preparation still uses CPU. |
| Transforms / resize | Affine sampling and native CMYK/Lab channels; immutable tile snapshots cache flattened sources, and exact upload caching reuses them across drags. | Free Transform raster/smart-object previews and Apply, plus classical Image Size, await GPU operations. Selection-transform and other synchronous library callers retain CPU execution in wasm. |
| Moving layers | Translated tile uploads are deduplicated within each output row; masks, negative coordinates and native channels retain parity. | Async composite snapshots preserve offsets. |
| RAW | Sensor normalization, white balance, padded CFA preparation, Bayer/generic demosaic, SuperCCD shear/rotation, crop, matrix and orientations; bounded final-stage bands; decoded-sensor cache; exposure histogram, shoulder and sRGB encoding. Camera Raw's remaining controls have a complete filter graph. | RAW preview and Apply await sensor development and encoding, with document/revision/sequence guards. Oversized resident browser developments fall back to CPU. Container parsing/decompression remain CPU. |
| Selection | Color Range, Similar, wand and Grow classification; atomic connected components, seed selection and mask extraction. Existing morphology/feather kernels remain available. | Wand, Grow/Similar and Color Range use owned asynchronous edits. Quick Selection's evolving mean and other synchronous gestures retain their existing algorithm. |
| Blur / sharpening / noise | Complete nondefault Lens Blur, including aperture/depth/noise; Reduce Noise, Smart Sharpen, Sharpen More/Edges, Blur/Blur More, Despeckle and Dust & Scratches. | Complete filter descriptors run through preview/Apply. |
| Distortions | Auxiliary-map Displace, GPU Twirl coordinates, and paged nonlocal sources. Ripple/Wave retain small separable coordinate tables for consistent transcendental rounding. | Captured maps, paths and toolbox colors survive asynchronous fallback. |
| Gallery / whole-image filters | Complete Artistic, Brush Strokes, Sketch, Texture and Pixelate families; Average, Custom convolution, HSB/HSL, Deinterlace, NTSC Colors, Diffuse Glow, Wind, Tiles and Oil Paint. | Filter Gallery composes supported operations into one resident graph; mixed unsupported stacks retain complete CPU fallback. |
| Procedural rendering | Lens Flare, Lighting Effects, Picture Frame, Bump/Normal Map, Tree, Flame and deterministic Extrude coverage/overlap resolution. | Filter descriptors capture context. Small tree geometry, path roots and aperture geometry are prepared on CPU. |
| Neural inference | Static shape folding, grouped ConvTranspose, normalization, reductions/pooling, general batched MatMul, Gemm, attention arithmetic, Slice/Gather/Expand/Split, multi-input Concat, multiple outputs and Resize variants. All bundled restoration/upscaling graphs compile, including Waifu2x. MiDaS depth, U2Net segmentation and MobileCLIP vision graphs were executed against tract. | Complete tiled RGBA descriptors cover JPEG repair, detail enhancement and compatible style-transfer models. Other neural hosts remain synchronous; token-input and unsupported ONNX graphs retain tract. |
| Color management | Additional CICP transfers, CMS-compatible smooth CLUT profiles, native profile conversion descriptors and proof/display graph composition. Lattice reconstruction preserves the CMS's quantized interpolation instead of adding another LUT approximation. | Canvas proof/display conversion and native preview conversion await GPU operations. Profiles that fail conservative eligibility/probe checks use the original CMS executor. |
| Healing / fill | Candidate patches stay in the exact upload cache; GPU scoring and stable minimum reduction read back one winning index per placement. Existing diffusion/seam passes retain intermediates. | Boundary priority and patch placement remain sequential. Browser synchronous retouch callers retain CPU execution. |
| Vector coverage | Flattened-edge raster coverage supports even-odd/nonzero fills and subpixel coverage. | Path flattening and text shaping remain CPU; browser synchronous vector preparation retains its current execution path. |

## Shared execution and browser editing

`ComputeProgram` supports element, RGBA, atomic-output and workgroup kernels.
Appending a graph remaps its inputs and intermediate references; dead outputs
are reused and only the final result is read back. `FilterOperation::Sequence`
composes Gallery stacks, while captured operations retain model/profile/context
data. The exact immutable upload cache confirms full byte equality after hash
lookup, so hash collisions or edits cannot reuse stale data.

Browser filters and owned tool edits each keep one running request and the
latest pending request. Results are installed only when the document, revision
and request sequence still match. Transform previews create no history entry;
Apply records the original snapshot once. CPU fallback uses the same captured
input. Merely exposing a compute kernel does not make a synchronous wasm caller
asynchronous.

Flattened PNG/JPEG/WebP/TIFF exports and artboard/slice exports await the
compositor against an immutable raster/profile snapshot. Encoding remains CPU.
Other synchronous library/plugin entry points and direct GPUI texture handoff
remain integration boundaries. The pinned GPUI dependency keeps its
renderer/device/atlas private and uses different
native rendering backends. Sharing those resources requires a GPUI API change;
completed canvas images currently retain the GPU→CPU→GPU transfer. This branch
does not replace that dependency or claim hardware performance improvements.

## Bounds and numerical contracts

- General programs have a **256 MiB aggregate allocation budget**, including
  inputs, retained intermediates, parameters and final readback. Binding sizes
  and dispatch limits are checked before submission. Cached buffers consume
  only the remaining budget, up to 64 MiB.
- Nonlocal effect shaders use texture-array source pages and bounded output
  bands when storage bindings cannot hold the source. This does not remove the
  aggregate budget or make arbitrary resident graphs unbounded.
- Flattened affine snapshots and their source tiles retain at most 64 MiB;
  decoded RAW caching retains one image, with a 128 MiB input/sample/preview budget. Tiled
  resident neural image graphs are bounded to 256 tiles and 16M output floats.
- Float parity includes HDR/transparent pixels, narrow images, paging boundaries,
  native channels, SuperCCD and all orientations. Most comparisons use `1e-4`;
  transcendental remaps use `5e-4` in premultiplied contribution. Neural/CMS tests
  use `3e-4`; mask/vector coverage allows one byte of rounding difference.
- Unsupported ONNX semantics reject the whole GPU graph. Integer shape data
  keeps its original width, including Slice sentinels. External tensor files,
  token inputs and dynamic/unsupported operators retain tract execution.
- Dispatch thresholds estimate work. Actual speedups and threshold tuning need
  hardware profiling that includes preparation, upload and readback. Software
  Vulkan/SwiftShader tests establish correctness, not acceleration performance.

## Verification

- `make check-gpu-fx`: native effects, resident programs, compositor and regression tests.
- `make check-gpu-opportunities`: real caller parity and remaining opportunity coverage.
- `make check-gpu-domains`: CPU/reference tests in the domain crates.
- `make check-web-gpu` / `make lint-web-gpu`: browser host builds and lint checks.
- `make test-web-gpu`: actual WebGPU execution, including atomic histograms,
  connected selections, transform/resize undo, Gallery, tiled neural filters and
  RAW development/encoding and float export compositing. GPU decline is a
  failure in these execution tests.

The optional downloaded-model regression uses `SCHIST_GPU_MODEL_DIR` containing
`depth.onnx`, `segment.onnx` and `embed-image.onnx`; setting
`SCHIST_GPU_MODEL_EXECUTE=1` also compares inference with tract. The default suite
uses committed small operator fixtures and bundled models without downloading.
The synthetic RAW fixture is generated by the DNG builder in
`plugins/codecs-common/src/raw.rs` with dimensions 67×65.

See [compute ABI](../crates/fx/src/compute.rs),
[browser scheduling](../crates/editor/src/workspace/browser_gpu.rs),
[filter descriptors](../plugins/filters-core/src/gpu.rs),
[ONNX compiler](../crates/neural/src/gpu.rs),
[ICC lattice conversion](../crates/colormgmt/src/gpu_lut.rs) and the
[ONNX operator specifications](https://onnx.ai/onnx/operators/index.html).

# GPU coverage and remaining opportunities

The compute backend now covers work in every major area identified in the
initial map. Native callers use the shared `schist-fx` backend; browser callers
must explicitly await an operation. Small workloads, unsupported operations,
device failures and jobs exceeding the memory budget retain CPU execution.

## Implemented coverage

| Area | GPU implementation | Remaining work in this area |
| --- | --- | --- |
| Adjustments | Every supported direct compositor adjustment, including ranged Hue/Saturation, Color Balance, Vibrance, Photo Filter, Gradient Map, Selective Color, Channel Mixer and White Balance. Variable coefficient records replace the fixed record limit. Destructive adjustments also have float-buffer kernels, including exact curves and levels formulas. | Auto-adjust histogram/statistics and asynchronous destructive-adjustment browser callers. Unknown/unsupported adjustment kinds retain existing behavior. |
| Moving layers | Integer render offsets sample up to four source tiles, including negative coordinates, masks and native CMYK/Lab. Browser snapshots preserve the offset. | Shader stack depth remains bounded. Tile uploads could be deduplicated across translated destinations. |
| Layer effects | Shared alpha blur, bilinear alpha offset and bounded signed-distance kernels accelerate shadow, glow, satin, stroke and bevel preparation. Mixed blur radii and CPU accumulation order are preserved. | Style assembly, bevel shading and overlay evaluation remain CPU stages. |
| Transforms / resize | Affine nearest, bilinear and bicubic sampling, minification box sampling and edge coverage; RGB, CMYK and Lab remain in their native channel layouts. Existing transform/resize/smart-object callers use this seam. | Source tiles are flattened/uploaded per operation; keep them resident across interactive drags. Browser synchronous transform callers still use CPU. |
| Blur variants | Spin, Path, Shape and Smart Blur have companion kernels. High Pass, Sharpen, Unsharp Mask and Field/Iris/Tilt-Shift Blur execute complete resident compute programs. | Nondefault Lens Blur apertures/depth/noise stages and additional compound artistic/sketch stages. |
| RAW development | Sensor normalization/white balance, Bayer Fast/Best and generic CFA Fast/Best demosaic, camera matrix, crop and all eight orientations. Demosaic processes bounded bands; internal passes remain on-device. | Decoding, padded-mosaic preparation, SuperCCD geometry and later Camera Raw filter stages remain CPU. Cache decoded sensor data across edits; stream large final matrix/crop jobs. |
| Browser callers | Async canvas composite and viewport rendering; preview/Apply operations listed below, including resident programs and captured filter context. Coalescing, cancellation, document/revision checks and one history entry per Apply. | Other synchronous tools, library APIs, export, RAW, neural inference and previews outside this host still use CPU in the browser. |
| Selection / masks | Feather, expand, contract, border, smooth and affine mask transforms, preserving canvas boundaries and byte rounding. | Color Range/Similar classification and connected wand/grow operations. |
| Distortions / lens | ZigZag, Spherize, Pinch, Polar Coordinates, Shear, Adaptive Wide Angle, Lens Correction including channel aberration and vignette, Glass and Ocean Ripple. | Auxiliary-map Displace; paged sources for nonlocal companion remaps. Existing Twirl still prepares offsets on CPU. |
| Compound stages / transfers | Generic float-buffer programs upload inputs once, keep intermediate passes on-device, reuse dead intermediate allocations and read only the final result. Used for compound blur, RAW, masks, diffusion and neural graphs. | Filter Gallery graph composition, remaining Reduce Noise/artistic/sketch stages, persistence across separate jobs and sharing GPUI renderer resources. |
| Neural inference | Checked ONNX subset: Conv, Relu, LeakyRelu, Sigmoid, Tanh, Add/Mul/Div broadcasting, 2D MatMul, Transpose, constant BatchNormalization, Identity, float constants, nearest asymmetric Resize, two-input Concat and Softmax. Detail, JPEG repair, Portrait, Colorize and Inpaint run whole graphs on GPU. An unsupported node rejects the GPU graph and keeps tract execution. | Other Resize/Concat variants, ConvTranspose, attention and operators needed by Waifu2x, segmentation, depth and embedding models. Browser host scheduling and persistent weights. |
| Color management | Matrix/TRC RGB ICC transforms, including sRGB/linear CICP transfer metadata; alpha is preserved. Parity cases include sRGB, Display P3, Adobe RGB and linear ACEScg. CPU/GPU matrix execution uses float arithmetic and CMS-compatible transfer tables. | CLUT profiles, other CICP transfers, native CMYK/Lab profile conversion and proofing pipelines. |
| Healing / content-aware fill | Iterative boundary diffusion and seam relaxation retain their planes on the GPU and reuse allocations between passes. | Candidate scoring/reduction and source-patch residency; patch selection/placement remains sequential on CPU. |
| Other whole-image effects | Mosaic uses cell reduction followed by broadcast; Crystallize, Pointillize, Mezzotint and Texturizer have complete GPU operations. Existing companion stages continue to accelerate other effects. | Remaining pixelate, brush-stroke, sketch, artistic and procedural lighting stages; prioritize using measured end-to-end costs. |
| Vector rasterization | Flattened-edge scan coverage for even-odd and nonzero fills, including subpixel vertical coverage. | Path flattening/text shaping remain CPU. Benchmark complex paths on hardware before tuning dispatch thresholds. |

Source entry points:
[compute ABI](../crates/fx/src/compute.rs),
[executor](../crates/compositor-gpu/src/exec/compute.rs),
[adjustments](../crates/adjustments/src/gpu.rs),
[alpha/mask programs](../crates/fx/src/plane.rs),
[affine transforms](../crates/core/src/resample.rs),
[RAW](../crates/codec-raw/src/demosaic_gpu.rs),
[neural compiler](../crates/neural/src/gpu.rs),
[ICC](../crates/colormgmt/src/gpu.rs),
[retouch](../plugins/tools-retouch/src/fill.rs),
[vector coverage](../crates/vector/src/raster.wgsl).
The neural compiler follows the [ONNX operator definitions](https://onnx.ai/onnx/operators/onnx__Conv.html)
and deliberately accepts only graphs whose complete operator sequence is supported.

## Browser whole-filter operations

The filter queue supports Gaussian/Box/Motion Blur, Add Noise, Median (including
large windows), Spin/Path/Shape/Smart Blur, High Pass, Sharpen, Unsharp Mask,
Field/Iris/Tilt-Shift Blur, ZigZag, Spherize, Pinch, Polar Coordinates, Shear,
Adaptive Wide Angle, Lens Correction, Glass, Ocean Ripple, Mosaic, Crystallize,
Pointillize, Mezzotint, Texturizer, Facet, Fragment, Find Edges, Trace Contour,
Emboss, Minimum, Maximum, Offset, Clouds, Difference Clouds and Fibers.

Descriptors represent the complete filter. Context-dependent operations capture
the toolbox colors before yielding; a failed job uses the same captured context
for CPU fallback. Merely implementing a kernel does not make a synchronous
browser caller asynchronous. See [browser integration](../crates/editor/src/workspace/browser_gpu.rs)
and [filter descriptors](../plugins/filters-core/src/gpu.rs).

## Bounds, accuracy and performance

- General programs have a 256 MiB aggregate allocation budget, including inputs,
  retained intermediates, parameters and final readback. Per-binding and dispatch
  limits are checked before submission. These programs do not page arbitrary
  large inputs; an oversized program falls back. Specialized warp/carve paths
  retain their existing texture paging.
- Median uses a small scratch array for radii 1–4 and constant-register radix
  selection above that. The median helper accepts radii through 100; the Median
  filter exposes its existing slider range. Large windows require more passes
  through their samples and are not a guaranteed speedup.
- CPU/GPU parity covers HDR and nearly transparent pixels, narrow images, band
  boundaries, native color modes and translated layers. Most shader comparisons
  use `1e-4`; new float remaps allow `5e-4` in premultiplied color contribution.
  Mask/vector coverage permits one byte of rounding difference. ICC and neural
  comparisons use `3e-4`. RAW demosaic uses `2e-5` and developed output `1e-4`, including a
  four-megapixel RAW case spanning two bands.
- GPUI owns a separate device. Completed canvas images still make a GPU→CPU→GPU
  trip. Device sharing, persistent transformed sources and cached neural weights
  remain opportunities beyond retaining intermediates inside a job.
- Offload thresholds estimate work; they are not measured speed guarantees.
  Hardware profiling, including preparation, upload and readback, remains
  necessary before lowering them. Small brush dabs and general UI/history/file
  parsing work remain CPU tasks.

## Verification

- `make check-gpu-fx`: native kernel, filter and compositor regression suite.
- `make check-gpu-opportunities`: adjustment coverage, real caller parity,
  resident filter programs and translated native/RGB composites.
- `make check-gpu-domains`: tests for the domain crates using the new seam.
- `make check-web-gpu` and `make lint-web-gpu`: browser integration builds/lints.
- `make test-web-gpu`: browser WebGPU execution, resident programs, context,
  translated snapshots, concurrency and cancellation. Requires a WebGPU-capable
  browser/WebDriver and does not accept CPU fallback as success.

The headless test adapter checks computation. Hardware acceleration performance
and a complete interactive canvas smoke test need a working hardware browser;
the container's SwiftShader canvas presentation is unreliable.

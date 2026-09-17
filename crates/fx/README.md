# Adding GPU effects

`schist-fx` dispatches whole-image work to the installed backend and keeps CPU
implementations as the reference. `GpuFx` shares the compositor's device.

## Give an effect a shader companion

1. Add a WGSL file beside the filter. Implement
   `fn effect(pos: vec2<i32>) -> vec4<f32>`; return straight-alpha RGBA.
2. Declare a static `ShaderSpec` with a diagnostic name and `include_str!` source.
   Built-in filters use the `shaders!` list in
   `plugins/filters-core/src/gpu.rs`, which also includes them in offline validation.
3. Normalize sliders in the effect's Rust body, then call `try_shader_rgba`
   **before changing its input**. Return if it succeeds; otherwise continue the
   existing CPU body. No new backend method or GPU pipeline field is needed.

```rust,ignore
static SHADER: schist_fx::ShaderSpec = schist_fx::ShaderSpec {
    name: "my-effect",
    source: include_str!("shaders/my-effect.wgsl"),
};

let radius = values.get("radius").round().max(1.0) as usize;
if schist_fx::try_shader_rgba(
    pixels, width, height, &SHADER,
    &[radius as f32], Some(radius), (radius * 2 + 1).pow(2),
) {
    return;
}
// Existing CPU implementation follows.
```

The shader receives:

| Name | Meaning |
| --- | --- |
| `pos` | Integer pixel coordinate in the full image, even in a band |
| `image.width`, `image.height` | Full image dimensions |
| `args[i]` | Packed f32 parameters, in the order Rust supplies them |
| `read_pixel(pos)` | Straight-alpha pixel, clamped at image edges |
| `premul(pixel)`, `straight(pixel)` | Explicit alpha conversion with the CPU's epsilon guard |
| `sample_premul(pos)` | Bilinear clamped sampling in premultiplied alpha, integer pixel centers |
| `sample_premul_offset(origin, delta)` | Bilinear sampling with integer coordinates kept separate from subpixel displacement |
| `luminance(pixel)` | The filters' existing luminance coefficients |
| `round_away(vec2)` | Rust-style rounding; WGSL's built-in `round` has different tie behavior |

The prelude supplies the entry point and four bindings: image dimensions,
read-only source, writable destination, and read-only parameter storage.
Call `ShaderSpec::wgsl()` to get the complete source for validation.

### Sampling range and cost

`halo: Some(n)` promises that no output samples more than `n` rows away.
Include the extra row read by bilinear interpolation. A horizontal or pointwise
operation uses `Some(0)`. Set `None` for unrestricted remaps such as Twirl or
Radial Blur. Those fall back to the CPU if the full source exceeds GPU limits.
Local effects split into overlapping bands and keep only the valid interior.
Never read `source` directly; the helpers account for each band's origin.
Use displacement sampling for remaps to avoid losing fractional bits when
adding a small motion to a large image coordinate. The Rust filter helpers
provide the matching `sample_offset` and `warp_offset` operations. Radial Blur
also prepares its sample rotations once and shares them between both paths.
Twirl, Ripple and Wave share CPU-prepared displacements with their shaders:
device-dependent trig and multiply-add rounding can otherwise move sampling
weights enough to change straight RGB at transparent edges. Ripple and Wave
only need one displacement per row/column; Twirl needs two floats per pixel.
The GPU still performs the premultiplied sampling. Twirl's map adds eight bytes
per pixel of upload/storage and is subject to the shader parameter binding
limit; an oversized job retains the CPU fallback.

`work_per_pixel` estimates source taps or equivalent arithmetic. The production
backend uses the existing conservative offload threshold; tiny effects stay on
the CPU. This estimate is not a speed guarantee. Measure end-to-end execution
including uploads and readback on a hardware adapter before tuning thresholds.

Pipelines compile lazily and cache by shader source, including failed
compilations. A broken companion cannot disable the compositor or other effects.
Validation, allocation, size-limit and readback failures decline the job; the
caller's original buffer remains available for CPU fallback. This API is for
trusted effect code, with valid parameter ranges and the same CPU semantics.

## Current companions

| Kernel | Filters using it |
| --- | --- |
| Convolution | Sharpen More (both 3×3 passes), Custom (5×5) |
| Morphology | Maximum, Minimum; square and round neighborhoods |
| Bilateral | Surface Blur, Reduce Noise's initial smoothing stage |
| Median | Median, Despeckle, Dust & Scratches |
| Blur sampling | Motion Blur, Radial Blur (spin and zoom), Fragment |
| Stylize | Find Edges, Trace Contour, Emboss, Oil Paint, Facet |
| Noise / remap | Add Noise, Offset, Twirl, Ripple, Wave |
| Procedural | Clouds, Difference Clouds, Fibers; toolbox colors included |

Median companions use scratch arrays for radii 1–4 and constant-register radix
selection for larger windows (the helper supports radii through 100). Oil Paint supports up to 64 intensity bins.
Gaussian/box blur, lens blur, mesh warp and seam carving retain their specialized
backends. Additional companions cover blur galleries, distortions, lens correction,
pixelate and texture effects; see the [coverage map](../../docs/gpu-opportunities.md).

## Resident programs and browser callers

`ComputeProgram` describes float-buffer steps with primary and auxiliary inputs,
owned parameters, output lengths and dispatch shapes. `ComputeSource::Step`
references an earlier result. Kernels implement `compute(index: u32)` over the
bindings in `COMPUTE_PRELUDE`; row/pixel kernels must guard their logical index
when they write more than one sample. The executor validates dependencies and
allocation limits, reuses dead outputs and reads back only the program result.
`FxBackend::compute_available(work)` lets callers avoid expensive preparation
when no eligible backend is installed. A declined program leaves the original
input available for CPU execution.

`FilterPlugin::gpu_operation_with` exposes a **complete** asynchronous operation,
including captured context. Native callers use the same descriptor when present.
`FilterOperation::Program` builds dimension-dependent graphs; High Pass, Unsharp
Mask, graded blurs and Mosaic use it. Browser hosts await the descriptor and
handle stale results, cancellation, fallback and history. A synchronous library
call in WebAssembly still uses its CPU implementation.

## Verification

Run `make check-gpu-fx`, `make fmt-gpu-fx`, and `make lint-gpu-fx`.
`make check-gpu-shaders` runs just the new companion execution tests.

Register new companions in the shader list and add real-filter cases to
`crates/compositor-gpu/tests/effect_shaders.rs`. Its forced backend verifies that
GPU work actually ran, compares it with the CPU body, and exercises HDR colors,
transparent and nearly transparent pixels, single rows/columns and band edges.
Near the unpremultiply cutoff, parity compares the pixels' premultiplied color
contribution; elsewhere it compares straight-alpha channels within `1e-4`, except new floating
remaps which allow `5e-4` in premultiplied contribution.
The shader list is parsed and validated without an adapter; execution tests need
a wgpu adapter and report a skip when none is available.

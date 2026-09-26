# Neural inference on the GPU

Catalog models use native GPU inference through the installed effects backend.
Compatible networks compile to resident compute graphs, including SFace
recognition's PReLU and inference Dropout layers. Anti-Smudge, the MobileCLIP
text encoder and unsupported background-matting graphs run their expensive
contractions on the GPU within a tract execution plan. This uses the existing
wgpu device, without a separate inference runtime or CUDA requirement.

Anti-Smudge has 279 eligible contractions and the text encoder has 29. These
counts describe graph coverage, not guaranteed dispatches: the production
backend keeps small operations on the CPU. Token IDs and indexing retain their
integer representation. Shape operations, unsupported kernels, image preparation
and Anti-Smudge's halo cleanup continue to use the CPU.

Large convolutions use spatial bands with exact padding, strides, dilation and
group boundaries. Each band preserves the original convolution's receptive
field. The model still sees its full 2048px input and full attention context;
inference does not substitute smaller image tiles or change the weights.
Each band holds at most 4,194,304 floats in its image and output buffers. Dense
convolutions use cooperative 32×64 output tiles with 2×4 register blocks per
thread; narrow convolutions retain 16×16 tiles and small grouped kernels use
the element shader. Matrix contractions use 64×64 output tiles with 4×4
register blocks, including strided/broadcast batches and compound reduction
axes. Unsupported contraction layouts retain the scalar kernel.
The offload threshold applies to the entire contraction so a
short final band cannot inadvertently force the whole operation back to CPU.

Without a GPU, Schist runs an optimized tract plan with native host kernels.
If a GPU operation fails or exceeds device limits, it runs through an independently
optimized tract fallback using its original inputs. A failure after a successful
band discards the partial output and recomputes the operation. No partial tensor
is passed to subsequent layers.

GPU eligibility alone is not a speed guarantee: partial graphs can spend more
time copying, indexing and rearranging tensors on the host than doing GPU math.
Large contiguous host matrix transposes use a cache-blocked copy, while moving
unit dimensions remains a zero-copy reshape. The release profile optimizes the
host tensor and upload/readback loops for speed.

The background detectors keep their data-dependent `Floor → Cast<int64>` pixel
indices as ordinary integers, avoiding tract's symbolic-dimension arithmetic.
Concrete float GatherND operations copy checked contiguous slices, including
batched and negative indices. Fixed linear resizes reuse tract's interpolation
plans and specialize two-tap contiguous rows without changing their accumulation
order. ARM64 transposes use blocked NEON bit shuffles with scalar tails; other
architectures retain the portable transpose implementation. These host passes
apply to both CPU and partitioned GPU plans, preserving model resolution and
precision.

Fixed float32 constant padding copies whole contiguous rows into a prefilled
output. Input and padding-value bits are preserved, including signed zero and
NaN payloads. Reflect/edge padding and unsupported shapes retain tract.

The foreground models also recognize the exporter's four-corner deformable
sampling subgraph. A fused operation performs the same gathers, weighted sum,
modulation and convolution-layout conversion directly. Sampling metadata is
prepared in blocks of 1024 pixels and reused across feature channels, avoiding
four large channel-expanded tensors and their transposes. The matcher checks
topology, shapes, kernel layout and ONNX domains; other consumers of the original
nodes remain live. Model weights and compressed ONNX files are unchanged.

On macOS, the background models' CPU plans use Accelerate vForce for contiguous
float32 softmax exponentials, with tract's SIMD sum and normalization. Stable
maximum subtraction is retained. Vector exp and reduction order can introduce
small rounding differences; this does not use reduced-precision weights or
approximate fast-exp softmax. Other platforms retain tract's softmax.

The macOS CPU plans for both BiRefNet detectors and ViTMatte also rewrite
compatible large float32 Einstein contractions to Accelerate `cblas_sgemm`.
Checked matrix views cover transposed operands/output, broadcast weights,
interleaved attention heads and contiguous compound reduction axes, without
packing copies. Shapes, strides, allocation extents and the 32-bit BLAS limits
are checked before dispatch. Each product must require at least one million
multiply-accumulates; small or unsupported contractions retain tract.
This changes execution kernels, not model weights, precision, trimaps or tile
geometry. Accumulation order may introduce small floating-point differences.

The native automatic background-removal action opts into measured placement.
On its first input it times both CPU and accelerated execution of each large
model family, including transfers and host operators. It uses CPU when at
least 20% faster; otherwise it keeps GPU offloading. Both pinned BiRefNet Lite
detectors share a calibration; ViTMatte calibrates independently on its first
tile. Choices are scoped by input shape and backend identity, retained across
model-plan releases, and reset when the backend is replaced or the app restarts.
The resident subject guide and small MatteNet remain eligible for GPU execution.
The scope changes neither the global backend nor unrelated filters, and raw
CPU/GPU parity tests bypass it. See [background-removal timings](background-removal.md#native-execution-performance).

The partitioned path uses the synchronous native effects backend. Browser
filters retain their existing asynchronous resident-graph path; partitioned
Anti-Smudge inference in the browser still uses tract. Compilation alone does
not imply that a browser or device can execute a model on the GPU.

## Verification

`SCHIST_MATTING_PROFILE=1 make profile-background-removal ARGS='cpu photo.jpg cutout.png'`
logs operator timings for each model's first CPU input. It does not record image
or tensor contents. `make profile-neural-tensors` compares portable and native
transposes on representative synthetic tensor sizes; timings are diagnostic,
not assertions in the regression suite.
For paired end-to-end comparisons, `SCHIST_NEURAL_LEGACY_HOST=1` selects the
previous host operators in the same executable. It is a native diagnostic
control read once per process; weights, precision and tiling stay unchanged.
`SCHIST_NEURAL_LEGACY_MODEL=1` separately disables deformable-sampling fusion and
the macOS vector softmax for comparisons with the preceding model execution.
`SCHIST_NEURAL_LEGACY_COMPUTE=1` disables the Accelerate matrix and contiguous
padding passes for paired comparisons with the preceding implementation.
`SCHIST_MATRIX_LAYOUTS=1` logs unsupported contraction equations and shapes
without tensor contents, to identify further packing/stride costs.

`make check-neural-gpu` executes real GPU dispatches and compares them with tract:
PReLU, inference Dropout, grouped/dilated/asymmetrically padded convolution,
broadcast/transposed matrix products, compound reductions, non-tile-aligned
dimensions, integer token lookup followed by matrix products, large convolution
bands (including the wider kernel), and failure before or during a GPU operation. It also
checks that every installed catalog model has a resident or partitioned path.
`make lint-neural-gpu` runs strict Clippy for both affected crates.

The small committed fixtures are generated by
`python3 tools/neural-gpu-fixtures.py` (requires NumPy and ONNX). Real downloadable
weights remain outside the repository. To test all catalog entries and run
parity for every downloadable model:

```sh
SCHIST_MODEL_DIR=/path/to/models SCHIST_GPU_MODEL_DIR=/path/to/models \
  make check-neural-gpu
```

The complete bundled Anti-Smudge network is compared against tract at a 128px
input using the exact compressed shipping weights. Its shipping 2048px contract
is loaded by the catalog coverage check, and the large convolution fixture
separately verifies banding. Software Vulkan can establish shader correctness
but does not measure hardware GPU speedup.

`make check-background-removal-gpu` adds GPU/CPU comparisons for the bundled
MatteNet, ViTMatte-S and semantic guide, using the production offload threshold.
It requires real dispatches, including ViTMatte's full 768px input. Optional
checks exercise both installed background detectors and full-resolution detail
matting on a local photograph:

```sh
SCHIST_BACKGROUND_GPU_DETECTORS=1 make check-background-removal-gpu \
  ARGS='installed_background_detectors --nocapture'
SCHIST_MATTING_INPUT=/absolute/path/to/srgb-photo.png \
SCHIST_MATTING_COARSE=/absolute/path/to/coarse.f32 \
  make check-background-removal-gpu ARGS='full_resolution_private_photo --nocapture'
```

The coarse buffer is row-major little-endian float32 alpha at the photograph's
dimensions. Optional fixtures remain local; these tests do not fetch or commit
photographs. `SCHIST_MATTING_INPUT` also supplies a real photograph to the
detector comparison when set. GPU parity verifies implementation consistency,
not segmentation quality or an artifact-free guarantee.
`SCHIST_BACKGROUND_REFERENCE_DIR` optionally supplies independent runtime
references as `foreground.f32` and `foreground-matting.f32`, using the same
full-size alpha format. This also checks the CPU graph against those outputs.

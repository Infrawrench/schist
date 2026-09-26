# Neural inference on the GPU

Catalog models use native GPU inference through the installed effects backend.
Compatible networks compile to resident compute graphs, including SFace
recognition's PReLU and inference Dropout layers. Anti-Smudge, the MobileCLIP
text encoder and unsupported background-matting graphs run their expensive
contractions on the GPU within a tract execution plan. This uses the existing
wgpu device. macOS background removal additionally uses Core ML through Rust
bindings and the statically linked `ort` dependency described below.

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

Windowed attention and split-head projections can also use Accelerate when
their left operand needs rearranging. A checked packing plan copies contiguous
feature blocks into at most 16 MiB of float scratch, reused across batches.
The weights and output still use direct views. This converts fragmented
contractions into large matrix calls while retaining the same reduction axes
and float32 values. Unsupported layouts and larger scratch requirements retain
tract. Operator profiles distinguish these calls as `PackedAccelerateMatMul`.

ViTMatte's three large, low-channel decoder convolutions use a bounded CPU
banding path on macOS. It packs the 3×3 neighborhoods directly from the original
feature map, supplies zeros at the image boundary and writes Accelerate matrix
results into the output planes with their original bias. This avoids a separate
full-size padded feature map. The float scratch is capped at 16 MiB and reused
across bands and batches. Bands follow whole image rows when possible; narrower
outputs use longer bands to amortize matrix-call overhead. Only fixed float32
NCHW, ungrouped, stride-one, dilation-one, same-padded 3×3 convolutions with at
least 65,536 spatial positions and 16–64 output channels qualify. The smaller
decoder stages retain tract after local benchmarks found mixed or slower
results there. This path changes execution, preserving the filters, biases,
resolution and model tile overlap. It is logged as `BandedAccelerateConv`.

On macOS, the three bundled background refiners use precompiled Core ML
archives through Rust `objc2-core-ml` bindings. The Mac executable embeds only
`detail-matting.mlmodelc.tar.xz`, `subject-guide.mlmodelc.tar.xz` and
`matting.mlmodelc.tar.xz`, plus `detail-matting-gpu.mlmodelc.tar.xz` on Apple
Silicon. Their ONNX archives remain in the repository for other platforms and
are excluded from Mac production builds. The float32 archives total
138,908,300 bytes; the Apple Silicon GPU variant adds 48,339,112 bytes.
Excluding their ONNX copies avoids another 136,175,436 bytes.
They target Core ML 5 / macOS 12 or newer; these compiled refiners are
unavailable on macOS 11. Runtime validation here uses macOS 15.6.1; Intel Mac
and Web are also compile-checked.

Archives are checked against pinned SHA-256 hashes and extracted to stable,
versioned paths under `~/Library/Caches/schist/neural/coreml-bundled-v1`.
Extraction checks every member name, size and digest, disallows links and
traversal, bounds decompression, and publishes the complete directory by rename.
Core ML GPU failures retry the full-precision compiled model on the CPU, so no
fallback ONNX copy needs shipping. Input/output shapes, strides and float32 values are
validated. Synchronous predictions own their input storage and serialize access
to each model. Idle models are released after five minutes; active callers keep
their ownership. CPU and GPU model caches are separate.

The two optional/downloaded foreground detectors use the statically linked
`ort` 2.0.0-rc.13 / ONNX Runtime 1.28.0 Core ML provider on Apple Silicon. Cargo
verifies its static archive at build time. No ONNX Runtime dylib is packaged or
loaded. The application invokes no Python or Swift process. Other neural
filters, Linux, Windows and Web retain the existing tract/wgpu paths.

Rust graph preparation is restricted to five pinned source hashes. Detector
bilinear sampling is expressed as `GridSample` with the same padded input,
coordinates, border behavior and modulation. Constant Gather folding preserves
float bits; relative-position Einsum becomes Transpose/MatMul; redundant batch
axes and scalar Gather layouts are rewritten without changing weights or
resolution. The matting detector and detail refiner each form one Core ML graph.
The general detector has three Core ML partitions with small host shape steps.
The offline subject-guide export expands its 20 HardSwish operations into
HardSigmoid/Multiply, uses BASIC optimization to avoid unsupported activation
fusions, and produces one complete Core ML graph.

Large inline float constants are stored in standard binary MIL weight blobs,
keeping their bits and reducing both loading time and cache size. The bundled
models are converted offline by `make export-background-coreml` (Apple Silicon,
Xcode and `coremltools==9.0` in `MATTING_PYTHON`). The Rust source exporter reads
ONNX from disk; enabling its optional feature does not embed it in the app.
`models/background-coreml.json` records archive/member hashes and provenance.
Downloaded-detector caches receive the same lossless weight-storage
transformation in Rust before an existing graph is loaded again. Their first
conversion/compilation can still be slow. The original downloaded ONNX files
are retained for tract CPU fallback; they are not part of the application bundle.

The Apple Silicon detail variant additionally uses mixed-precision arithmetic:
layer normalization, reductions, softmax, powers, reciprocal square roots,
division and the final sigmoid stay float32, with other eligible operations
lowered to float16. Input/output remain float32. Core ML uses CPU+GPU with
low-precision GPU accumulation disabled. The original float32 detail archive
is retained for CPU fallback and Intel Macs: the mixed graph had excessive
CPU error on both stress inputs and a real portrait. GPU-load failure is
explicitly tested to select the float32 graph. The guide, opaque-core model
and detectors retain float32 execution. No image-resolution reduction is used.
`export_coreml.py --gpu-only` preserves verified float32 archives when updating
the GPU variant; their original source pins and archive hashes must match.

Plane inputs are copied directly into tract tensors; contiguous Core ML output
uses a bulk copy after validating capacity and strides. Padded/transposed
outputs retain checked logical-element access without reading padding.

Source pins and versioned cache paths isolate derived graphs. Prepared ONNX
files are checked against their digest before reuse. At most five pinned native
sessions can be retained; a five-minute idle timeout also releases their validated
source buffers. `SCHIST_NEURAL_CACHE` overrides the cache root.
`SCHIST_NEURAL_LEGACY_NATIVE=1` bypasses the detector Core ML path for diagnostics;
`SCHIST_NEURAL_COREML_CPU=1` forces compiled refiners onto the CPU.

`make check-background-removal-native` compares the two installed detectors
against original CPU ONNX graphs and all three compiled refiners against the
original tract CPU plans, on both Core ML GPU and CPU. It is an opt-in Apple
Silicon check. Ordinary tests cover graph constants, archive integrity, tensor
layouts, malformed inputs and idle cache ownership.
Float32 models retain the original maximum-alpha-error limit of 0.0005.
The intentional GPU quantization has separate limits, expressed in 8-bit
alpha levels: mean error below 0.5, 99th percentile below 2, maximum below 8.
Full-image phone-photo checks supplement these synthetic numerical checks.
`make check-background-coreml-bundle` inspects the release
executable for its architecture's compiled archives, absence of their ONNX payloads,
and absence of an ONNX Runtime dylib dependency. Mac packaging runs this check
on the stripped shipping copy before signing the application bundle.

The portable automatic background-removal path opts into measured placement.
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
Adding `SCHIST_MATTING_PROFILE_NODES=1` to the model profiler also reports the
40 slowest nodes with their names and input shapes. It records no tensor values.
`make profile-attention-matrices` compares optimized tract contractions with
packed Accelerate plans on the three production ViTMatte attention layouts.
It uses identical synthetic tensors and constant weights, warms both plans,
then reports medians from 12 alternating measurements per implementation.
Packing is included in the measured time.
`make profile-decoder-convolutions` compares the selected banded decoder layers
with optimized tract plus the existing fast-padding pass, using synthetic tensors
(including constant filters and bias) and eight alternating measurements per
implementation after warm-up and a
numerical comparison. It also identifies the layers kept on tract.
For paired end-to-end comparisons, `SCHIST_NEURAL_LEGACY_HOST=1` selects the
previous host operators in the same executable. It is a native diagnostic
control read once per process; weights, precision and tiling stay unchanged.
`SCHIST_NEURAL_LEGACY_MODEL=1` separately disables deformable-sampling fusion and
the macOS vector softmax for comparisons with the preceding model execution.
`SCHIST_NEURAL_LEGACY_COMPUTE=1` disables the Accelerate matrix, decoder
convolution and contiguous-padding passes.
`SCHIST_NEURAL_LEGACY_PACKING=1` disables only the additional packed-input matrix
path, retaining the preceding direct-view matrix and padding optimizations.
`SCHIST_NEURAL_LEGACY_CONV=1` disables only the banded decoder convolutions,
retaining the preceding matrix, packing and padding optimizations.
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

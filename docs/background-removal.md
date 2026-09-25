# Automatic background removal

Select an unlocked RGB pixel layer and choose **Layer → Background Eraser**.
This is the automatic layer action; the toolbar's Background Eraser remains
the existing brush. On first use, the model manager opens if BiRefNet Lite is absent.
Download **BiRefNet Lite**, then invoke the layer action again. Photographs are
processed locally; model setup downloads the required weights.

The result is an editable layer mask. Existing enabled masks multiply with it.
Background color mixed into translucent edges is estimated and removed. When
edge colors change, the result goes onto a duplicate layer and the untouched
source layer is hidden beneath it. Source alpha and the selection are preserved.
One undo restores the source and its visibility. Fully opaque colors and
already-transparent source pixels retain their original RGB. Escape cancels the pending application; a document
edit or switch to another layer also invalidates the pending result. The
detector itself cannot be interrupted mid-inference, but refinement checks
cancellation between tiles. The action currently supports native builds and
up to 16,777,216 pixels in the visible raster bounds. It does not run on groups,
CMYK/Lab documents, locked layers, or a transient uncommitted drag.

For further adjustments, use **Select → Refine Mask**. Automatic color cleanup
changes foreground color only; it never thickens the mask or fills hair gaps.

## What was trained

The pipeline combines pretrained detection, semantic guidance and alpha matting.
Pipeline revision 9 uses **ViTMatte-S** for native-resolution hair/fur details.
Native builds embed `detail-matting.onnx.xz` (95,638,600 bytes), the semantic
guide (40,454,876 bytes) and MatteNet (81,960 bytes). These XZ archives expand
in memory when their inference plans are constructed; raw ONNX copies are not
shipped. The locally trained MatteNet supplies broad opaque interior seeds.
Model cards record both archive and expanded hashes, checked by regression tests.
Web builds serve compressed assets on demand, following the existing model loader;
the automatic layer action currently remains native-only.

The general detector is downloaded separately. The stronger matting detector
used in the photo audit is an **optional pinned local export**, not a bundled
weight or an arbitrary model import. To reproduce that combined detector path:

```sh
uv pip install --python target/background-removal/venv/bin/python \
  -r tools/train/foreground-export-requirements.txt
make export-foreground
```

This verifies the upstream source and weights, converts them to ONNX, checks
PyTorch/ONNX parity and installs the pinned 223,559,431-byte model. It is listed
as **BiRefNet Lite Matting** in the model manager. Both this model and the
general **BiRefNet Lite** must be installed for the combined path. Other
installations retain the downloadable general detector. Large pretrained
weights are stored in the model directory, not embedded in the binary.
The detail refiner and support weights already ship with the app.
`make export-detail-matting` reproducibly regenerates the bundled XZ artifact
from its pinned upstream revision, verifies PyTorch/ONNX parity and writes no
raw ONNX file. `ModelSource::Generated` identifies the optional detector export;
its SHA-256 is checked on load and it has no import or download action.

1. The pretrained [BiRefNet Lite Matting](https://huggingface.co/ZhengPeng7/BiRefNet_lite-matting)
   predicts soft foreground alpha. Its upstream training emphasizes people,
   animals, hair and fur. This is a pretrained model conversion, not a new
   Schist training run. Source revision, hashes and conversion parity are in
   [`foreground-matting.json`](../crates/neural/models/foreground-matting.json).

2. The pretrained [BiRefNet Lite](https://github.com/ZhengPeng7/BiRefNet) identifies
   salient foreground at 1024 × 1024. Its 224,005,088-byte download is pinned by SHA-256 and is
   separate from the small U2NetP model used by Object Selection.
3. The built-in **DeepLabV3 Subject Guide** uses
   [torchvision MobileNetV3 weights](https://docs.pytorch.org/vision/2.0/models/generated/torchvision.models.segmentation.deeplabv3_mobilenet_v3_large.html)
   trained upstream on COCO/VOC labels. It recognizes people, birds, cats, cows,
   dogs, horses, and sheep. Reliable connected subject regions guide removal of
   unwanted salient objects, with a margin for hair/fur. It accepts multiple
   supported subjects instead of choosing only the largest. It only reduces
   alpha: even high semantic confidence cannot fill real gaps between strands
   or limbs. If no substantial subject is recognized, the pipeline keeps generic
   salient-object removal.
4. The pretrained [ViTMatte-S](https://github.com/hustvl/ViTMatte) replaces the
   local residual refiner in native builds. Full-resolution comparisons traced
   web-like hair clumps to the old refiner pushing soft detector alpha toward
   opaque/transparent extremes. ViTMatte receives a three-valued trimap:
   foreground where the 9×9 minimum exceeds 0.98, background where the 9×9
   maximum is below 0.02, and unknown elsewhere. Its 768-pixel input tiles
   discard 128 pixels of context on each side; the central 512-pixel windows
   advance 384 pixels and blend through their 128-pixel overlaps. This matters
   because transformer predictions depend on distant context, unlike the
   previous local convolutional model. Known regions remain exactly 0/1.
   To avoid thinning opaque sleeves or hands, additional foreground seeds come
   from MatteNet regions whose 33×33 minimum exceeds 0.98. Those cores expand
   by 12 pixels, leaving four pixels of unknown boundary inside the original
   opaque region. Thin clumps cannot supply these seeds; the operation stays
   within the original opaque hint and never overrides known detector background.
   This safeguard was added after an unseeded trial faded a sleeve and hand.
   RGB normalization follows the authors' ImageNet mean/std, embedded in ONNX;
   the mirror's default 0.5/0.5 processor is not used. Provenance and export
   parity are recorded in [`detail-matting.json`](../crates/neural/models/detail-matting.json).
   This is a pretrained model conversion, with no private-photo fine-tuning.
5. **Schist MatteNet**, the earlier fallback, is a new 4-channel convolutional network trained in
   Python/PyTorch on original RGB pixels, coarse alpha, and reference alpha.
   Five 24-channel hidden convolutions use dilation 1, 2, 4, 2, 1; an output
   convolution predicts a residual correction. Its 11-pixel context radius
   fits inside the 16-pixel halo discarded around every 128-pixel inference
   tile. The output stays soft, with no binary threshold or largest-component
   filter that would automatically throw away smaller subjects. Revision 2
   continuously fades residual corrections within 0.05 of certain alpha values;
   exact zero and one remain unchanged even in a tile containing an edge.

The two detector outputs are checked against the semantic guide at 520 × 520.
New foreground is allowed within a three-pixel radius of general-detector
support, or a nine-pixel radius of semantic confidence above 0.8. The union is
softened with sigma 1, then multiplied into the matting prediction. This keeps
new clothing when independently recognized and suppresses new furniture
fragments. It cannot add alpha where the matting detector predicts none.
A tighter semantic allowance was rejected after pixel inspection showed it
cutting a real flyaway-hair loop while only narrowing the furniture fragment.
With no substantial recognized person/animal, generic removal uses the general
detector. The existing connected-subject and cropped-frame safeguards then run.

Foreground-color estimation uses two local box-filter passes (radii 45 and
3), following [Forte, Approximate Fast Foreground Colour Estimation](https://github.com/Photoroom/fast-foreground-estimation)
and the implementation used by BiRefNet. It processes 256-pixel blocks with
48 pixels of overlapping context, preserves alpha exactly, and keeps RGB unchanged where the saved 8-bit
mask is fully opaque or hidden. It also avoids recoloring HDR samples or
pixels with existing source transparency. It reduces colored and light fringes when
the cutout is placed on a new background. It cannot remove an opaque object
incorrectly included in the mask.

Training uses L1 alpha error weighted toward incorrect coarse-mask pixels and
an image-gradient loss. Coarse masks are synthetically degraded with small
erosion/dilation, blur, and downsampling. RGB exposure/color augmentation,
flips, rotations, and two crop scales add variation. RGB/alpha pairs are
already composited photographs, so their edge colors are **not** passed off as
pure foregrounds for fake compositing augmentation.

Revision 2 additionally varies foreground confidence and faint background
leakage over smooth spatial fields. This addresses the uncertain, textured
interiors found when reviewing real phone photographs, which simple blurring
of a reference matte did not represent. The photographs were used for local
evaluation; no private photographs were used to train the shipped weights.

Pipeline revision 3 antialiases image reduction using a triangle filter whose
footprint widens with the reduction ratio. The previous direct bilinear sampling
aliased camera texture and caused a full-resolution clothing omission. Native
resampling is checked against Pillow float-image reference values.
An intermediate semantic-interior fill was rejected after full-resolution
inspection showed it filling a real gap between hair strands. Python and Rust
regression tests require the guide to preserve such gaps and translucent edges. A second safeguard preserves detector regions that extend to the frame edge and connect to a recognized subject; this prevents a backlit cap from being clipped by the lower-resolution semantic guide. Detached border objects remain excluded.

BiRefNet uses ImageNet channel normalization on sRGB and sigmoid on its output
logits **before** bilinear interpolation. It does not use IS-Net's per-image
peak division or stretch the output range. Its upstream MIT notice is in
[`BiRefNet-MIT.txt`](../crates/neural/models/licenses/BiRefNet-MIT.txt).
The matting exporter also retains the
[deformable-convolution exporter attribution](../crates/neural/models/licenses/deform-conv2d-onnx-exporter-NOTICE.txt).
The native loader fixes tract 0.23.5's batched GatherND shape rule and avoids
its preliminary slice rewrite, which otherwise crashes on this model's index
tensors. The worker releases each detector/guide plan after its stage finishes. The
44,091,283-byte semantic guide is embedded as a 40,454,876-byte XZ archive.
All background models participate in the existing resident/partitioned GPU
runtime. Expensive convolutions and matrix products use the installed wgpu
backend; unsupported operations and failed dispatches retain CPU fallbacks.
The BiRefNet graph uses codegen optimization in both plans to avoid tract's
preliminary `PushSliceUp` failure on its batched index tensors.
Its export, upstream weight hash, classes and license are recorded in
[`subject-guide.json`](../crates/neural/models/subject-guide.json) and
[`Torchvision-BSD-3-Clause.txt`](../crates/neural/models/licenses/Torchvision-BSD-3-Clause.txt).
It is pretrained; no additional private-photo gradient training was performed
for pipeline revision 3. The trained MatteNet revision-2 weights are unchanged.

## Data and provenance

The data is [MicroMat-3K](https://huggingface.co/datasets/merve/MicroMat-3k),
linked by the [authors of ZIM](https://github.com/naver-ai/ZIM#dataset-preparation).
Attribution: **Beomyoung Kim et al., NAVER Cloud (2024), hosted by merve**.
The pinned dataset card specifies **CC BY 4.0**. This concerns the dataset;
the separate ZIM implementation and model are not used.

The fetcher pins revision `a950e12d008de146b857227683123f2ba0ef0665`, records
every file URL and SHA-256, and downloads at most six instance mattes per
source image. There are 237 source photographs and 1,126 selected mattes:
835 training, 141 validation, and 150 test. All annotations and crops from the
same photograph stay in the same split (177 / 30 / 30 source photographs).
No source-image hash occurs in more than one split.

These are **instance mattes**, not labels for all foreground in a scene. They
supervise local refinement, not the semantic foreground detector. MicroMat-3K
is normally an evaluation set; using our own training split means these
results must not be represented as official MicroMat-3K benchmark results or
as zero-shot performance on that dataset.

The committed model report records the seed, environment, training duration,
selected checkpoint, source split IDs, validation history, held-out test
metrics, export hash, and ONNX parity error. Downloaded images and PyTorch
checkpoints live under ignored `target/background-removal/`.

The included 21,937-parameter fallback was trained for 2,500 steps, then fine-tuned
for another 1,500 steps with confidence-error augmentation on the M4's MPS
device. On 300 held-out crops (30 source photographs), synthetic coarse-mask
MAE decreased from **0.04858 to 0.02517**; gradient MAE decreased from
**0.01179 to 0.00889**. On the additional synthetic confidence-error stress test,
the previous refiner's MAE was **0.03780**, versus **0.02547** for revision 2
(32.6% lower). These numbers are in alpha units from 0 to 1.
The new PyTorch/ONNX maximum absolute difference was **4.18 × 10⁻⁷**.
These are local refinement results, not full-image foreground accuracy.

See [`matting.json`](../crates/neural/models/matting.json) for the complete run
report and [`matting-manifest.json`](../tools/train/matting-manifest.json) for
the exact downloaded files and attribution.

The original IS-Net pipeline's timings do not apply to BiRefNet. The new
detector is more expensive and can take substantially longer under memory
pressure. Python's BiRefNet session disables its allocation arena and memory
pattern cache. Batch review computes and saves the semantic probabilities
before loading BiRefNet, so both model plans do not remain resident together.
Python refinement assembles only one padded tile at a time, avoiding a full-resolution four-channel padded copy. Batch review releases each photo before decoding the next. Avoid running large training/build jobs during a batch review.

## Local phone-photo audit

A development audit compared 40 selected NYC-area iPhone photographs on white
and black backgrounds: portraits, backlighting, windblown hair, crowds, and
scene/object controls. The selection is portrait-heavy and contains no useful
dog/cat test coverage. Private photos were not used for gradient training.

Sparse, visually checked rectangles on 29 photos measure certain interiors
only: 29 opaque foreground rectangles and 28 background rectangles, with each
rectangle weighted equally. Corrections to ambiguous annotations were recorded
locally. All images were reviewed during development, so this is not a blind
benchmark, full-image ground truth, or a hair-boundary accuracy measurement.
At matching 1600-pixel previews, alpha MAE was:

| Pipeline | Opaque foreground | Background |
| --- | ---: | ---: |
| Original IS-Net + refiner | 0.27627 | 0.07924 |
| IS-Net + revision-2 refiner | 0.20770 | 0.08967 |
| BiRefNet Lite + revision-2 refiner | 0.04067 | 0.09700 |

The large improvement is preserving opaque faces, shirts, jackets, and hats.
Background error increased: some flags, signs, and nearby objects remain.
A distant crowd and tiny wildlife were missed. Fine hair can retain color
fringes or lose faint strands. One reclining portrait also lost dark clothing
when inferred from the original instead of the 1600-pixel preview. A separately
labeled alternate uses the preview coarse mask with full-resolution refinement;
it is not included in the raw model accuracy claim. Resolution sensitivity and
these semantic errors are remaining failures, not resolved general cases.

Revision 3 also checks resolution sensitivity on the two known failure photos.
Comparing original-size and 1600-pixel input detector probabilities at a common
1024 × 1024 grid, antialiasing reduced mean absolute change from 0.07737 to
0.00014 on the reclining portrait and from 0.03261 to 0.00388 on a backlit
portrait. This is a two-image development consistency check, not an accuracy
benchmark. Full-resolution clothing was recovered without substituting a
manually selected preview mask.

For the final revision-3 exports, the same sparse rectangles were compared at
matching final dimensions (39 original-resolution images and one reduced
panorama). Opaque-foreground MAE was essentially unchanged (**0.04081 to 0.04073**);
background MAE changed from **0.09707 to 0.00603** (93.8% lower). These are
region averages on the development photos, not a percentage of all artifacts
removed. The development gallery retained every raw output and marked failures; generated review images were later removed to reclaim storage.

Sign letters and most detached flags/fence are removed, and the full-size
clothing omission is fixed. Remaining failures include cloth beside an arm,
fragments touching hair, incomplete cropped clothing, tiny wildlife and distant
crowds. The guide also trims a held object in a statue control. Four public animal
checks are recorded separately: a dog, bird and paired canids from the existing
refiner validation/test splits, plus a [cat photograph by Alvesgaspar](https://commons.wikimedia.org/wiki/File:Cat_August_2010-4.jpg)
under CC BY-SA 3.0. These are visual development checks, not broad pet evaluation
or a claim of unseen data for the upstream models.

For revision 2, the native tract detector and Python ONNX Runtime were compared on the
same 1024 × 1024 sRGB input: one pixel differed by one 8-bit alpha level; all
other pixels matched. This verifies that detector input/output path on one
image, not every native layer action or scene.

For the final revision-3 pipeline, the complete native detector, subject guide
and refiner were compared with Python on a 2316 × 3088 sRGB camera image. RGB
matched exactly. Nine of 7,151,808 pixels differed by one 8-bit alpha level;
all other alpha values matched. The run took 52.96 seconds including model
loading and PNG output on the M4 under memory pressure. This is one-image
validation, not a general runtime or accuracy benchmark.

Revision 4 reprocesses the same 40 images with the matting detector, detector
agreement and edge-color estimation. On those same sparse regions,
opaque-foreground MAE decreases from **0.04073 to 0.00670** (83.5% lower).
Background MAE changes slightly from **0.00603 to 0.00611**. The cropped black
clothing is recovered, the red heart-sign fragment beside hair is removed,
and the detached leaf fragment between strands disappears. Some touching
cloth/furniture and flag material remain, as do incomplete distant subjects.
One hair-gap crop admits faint background haze that the older cutout cleared.
An enclosed-hole correction was rejected after it produced uneven holes in
backlit material while leaving adjacent haze.
The public cat check still misses faint outward whiskers.

On 24 separately generated synthetic composites with known foreground colors
and soft alpha, foreground-color estimation reduces fractional-edge composite
MAE from **0.03320 to 0.00341**. This controlled color-spill test does not measure
real-photo segmentation or alpha accuracy. Alpha is unchanged by color cleanup.

The revision-4 native path also validates both detectors and foreground-color
estimation against Python on a 2316 × 3088 source. Twenty of 7,151,808 alpha
pixels and thirteen RGB pixels differ by one 8-bit level; all other values
match. Loading, processing and PNG output took 124.37 seconds on the M4 under
memory pressure. This remains a one-image implementation check.

Revision 5 uses pipeline revision 9 for the same 40 full-size exports and four
public animal checks. The supplied enlarged hair example was matched to photo
285. Comparing the exact crop on white and black shows substantially less
web-like opaque clumping and more continuous translucent strands. Sixteen
native-resolution crops also check windblown hair, backlighting, sleeves,
clothing and touching objects. A broad opaque-core safeguard prevents the
severe sleeve fading found in an unseeded trial; the already-incomplete hand
in photo 010 still has a softer boundary than revision 4. Fine whiskers, faint
flyaways and background haze remain limitations.

On the same 57 sparse certain-interior rectangles, opaque-foreground MAE is
**0.00659** versus **0.00670** previously; background MAE is **0.00626** versus
**0.00611**. This confirms approximately stable certain interiors, not a
measured real-photo hair improvement. On eight separate procedural hair-like
coverage scenes, boundary-band alpha MAE decreases from **0.17925 to 0.03635**,
and gradient MAE from **0.02553 to 0.00441**. These scenes have known compositing
alpha and synthetically degraded coarse masks; they are not real-image ground
truth and were not used for training or parameter selection.

All 40 original hashes, output dimensions, saved masks, source-color preservation
at opaque/hidden pixels, sRGB tags and metadata checks pass. One first-pass
batch export had a horizontal corruption line and failed color preservation;
it was rejected and replaced with a full CPU rerun. A fresh full-image MPS rerun
matches that replacement within one byte per channel. The exact cause of the
first export corruption was not established. These photo-audit numbers use
Python ONNX Runtime and native CPU references. GPU integration was added after
this audit and is validated separately below.

Before GPU integration, `make check-background-removal` passed 69 tests
(13 Python, 4 core, 31 neural unit and 21 inference tests), plus the editor
all-targets check and strict internationalisation audit. The additional pretrained refiner is
substantially slower than the small local fallback; gallery timings that reuse
detector caches do not represent complete layer-action latency.

The final native detail path matches Python ONNX Runtime on photo 285
(2316 × 3088): 30 alpha pixels and 15 RGB pixels differ by one 8-bit level;
all other values match. Model loading, opaque-core and detail refinement,
foreground-color estimation and PNG output took **86.38 seconds** on the M4.
This comparison reuses the previously validated detector/guide output and
excludes their inference time. It is an implementation check on one image.

## Native GPU integration checks

After rebasing onto the native GPU pipeline, `make check-background-removal`
passes 74 tests (13 Python, 4 core, 36 neural unit and 21 inference tests) and
the editor all-targets check. `make check-background-removal-gpu` verifies real
GPU dispatches for the bundled refiners and guide at the production offload
threshold, alongside the existing catalog, Anti-Smudge and fallback checks.
Strict `make lint-background-removal`, `make check-web-gpu` and the
internationalisation audit also pass. The final native release app builds with
`make app`.
Both installed BiRefNet detectors also execute 422 GPU dispatches on the
real-image check. Their decoded alpha matches native CPU exactly for this
input and differs from the independent Python references by at most 0.00001163
(general) and 0.00001413 (matting). This additional reference comparison guards
against errors shared by the native CPU and GPU graph paths.

On the same 2316 × 3088 source and cached detector/guide alpha, native GPU
detail refinement makes 3,338 dispatches and differs from native CPU by at
most **0.00000596** in float alpha. Against the earlier Python export, only
29 of 7,151,808 saved alpha pixels differ, each by one byte. The supplied hair
crop was rechecked on white and black backgrounds. This validates the GPU
implementation of the accepted refinement; it is not a repeat of the entire
40-photo audit on GPU, an accuracy benchmark or a speedup claim.

## Reproduce

```sh
uv venv --python 3.13 target/background-removal/venv
uv pip install --python target/background-removal/venv/bin/python \
  -r tools/train/matting-requirements.txt
make matting-data
make train-matting
make export-subject-guide
make check-background-removal
```

Training selects the checkpoint using validation MAE only, then evaluates the
held-out test split once. The default is 2,500 Adam steps, batch 16, seed 7,
using MPS on Apple Silicon (CUDA or CPU elsewhere). The generated model,
checkpoint, JSON report, and test contact sheet go into
`target/background-removal/`. The contact sheet columns are original RGB,
reference alpha, degraded coarse alpha, and predicted alpha, shown on a
checkerboard. To ship a retrain, replace `crates/neural/models/matting.onnx.xz`,
its report and reference output, and update the catalogue's byte count.

To reproduce the second-stage fine-tuning from the original checkpoint:

```sh
make train-matting MATTING_OUT=target/background-removal/matting-v2.onnx.xz \
  MATTING_STEPS=1500 \
  MATTING_ARGS='--resume target/background-removal/matting.pt --detector-errors'
```

The original checkpoint must be preserved before retraining; its SHA-256 is
recorded in the revision-2 report. The architecture's confidence fade is part
of revision 2, so the current source does not reproduce the original model bit for bit.

Python inference, after exporting the optional matting detector and downloading the pinned general detector:

```sh
target/background-removal/venv/bin/python tools/train/remove_background.py \
  photo.jpg --out cutout.png --coarse-out detector-only.png
```

This writes new PNGs and refuses to overwrite existing files. HEIC orientation
and embedded color profiles are respected; tagged colors are converted to
sRGB before inference, with alpha preserved. Outputs are tagged sRGB without
copying camera EXIF or GPS. Model paths can be specified with `--detector` and
`--refiner`; the default detector is `target/background-removal/foreground-matting.onnx`.
The default refiner is `crates/neural/models/detail-matting.onnx.xz`.
Pass `--refiner crates/neural/models/matting.onnx.xz` to compare the older model.
The cross-check uses `target/background-removal/birefnet-lite.onnx`
(`--reference-detector` changes its location).
Use `--no-color-cleanup` to inspect the mask with original RGB, or
`--no-detector-agreement` to ablate the detector cross-check.
Use `--no-subject-guide` to compare salient-object removal without
semantic guidance. The old detector remains available using `--detector-kind isnet`
and its matching file. Native inference can be checked
with `make background-removal-example ARGS='photo.jpg cutout.png'`; the detector
must be installed in Schist's model directory or `SCHIST_MODEL_DIR`.

For a local regression gallery, prepare a private JSON manifest containing
`[{"id":"case-1","image":"/path/to/photo.heic"}]`, then run:

```sh
target/background-removal/venv/bin/python tools/train/review_background.py \
  --manifest cases.json --out review \
  --detector target/background-removal/foreground-matting.onnx \
  --detector-kind birefnet-matting
```

The gallery records the pipeline revision and guide hash, so an interrupted
older pipeline cannot be resumed under different preprocessing. It offers
white/black/gray backgrounds, PNG cutouts and masks, source
hashes, model hashes and per-image timings. `--before` adds the previous
cutouts with matching case IDs. `raw-detector/` stores the detector's original
probability maps before semantic guidance; `coarse/` stores the guided masks
before refinement. The corrected revision-3 export uses provenance schema
`pipeline_revision: 5` to prevent resuming earlier guide rules. The new pipeline
uses revision 9 and records both detector hashes, both refiner hashes and the color-cleanup setting.
`reference-detector/` saves the original general-detector probabilities.
`--guide-cache`, `--reference-cache` and `--detector-cache` reuse compatible
predictions, with model and processing provenance checks. Detector/reference
caches also verify original source hashes. Timings identify reused predictions
and describe the current processing pass.
The default long edge is 1600 pixels for review;
`--long-edge 0` processes original dimensions. `--resume` verifies provenance
before continuing an interrupted run. Keep private manifests and photos outside
version control; the gallery operates locally and makes no upload requests.

## Quality limits

There is no guarantee of artifact-free removal on arbitrary backgrounds.
The trained refiner can sharpen a plausible boundary; it cannot recover a
person or pet missed by both the foreground detector and semantic guide. Small distant
subjects, crowds, hair against similar colors, fur, transparent objects, and
background color spill remain difficult. The detector can also retain
unwanted salient objects. Whole-image semantics and alpha matting are separate
problems, and synthetic coarse-mask metrics only measure the latter.

For this reason, retain the editable mask and inspect important results on
both light and dark backgrounds. More diverse people, dedicated pet scenes,
and training on labeled real detector failures are still needed before making
a production-quality claim across those cases.

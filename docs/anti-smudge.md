# Anti-Smudge (experimental)

Anti-Smudge appears in **Filter → Neural Filters**. It is intended to reduce
light streaks and scattering haze from a contaminated camera lens while retaining
real light sources. It is a restoration CNN, not a generative image editor.

**The selected MFDNet transfer model ships with Schist**, stored as
[`anti-smudge.onnx.xz`](../crates/neural/models/anti-smudge.onnx.xz).
Native builds embed the compressed bytes and expand them in memory only when
loading the model. The inference plan is cached; no uncompressed file is written
and no build-time decompression or external model installation is needed.
The web build serves the same compressed file alongside the application and
unpacks it after fetching it through Manage Models. The filter remains
experimental: it reduces streetlight halos, but window streaks and other blur
can remain. Source photos, datasets and intermediate checkpoints stay untracked.

The archive is 23,504,872 bytes and expands to the exact 27,185,281-byte ONNX used
for the latest small-halo preview. Its compressed SHA-256 is
`e8a9fa635ecf5bdbedad4944ad7a776d36b12d658dca7e994f73bc408cd2a875`;
the ONNX SHA-256 is
`553903af0e612c959d087b35cbbcbd00a0d4e890f7c1cc87ef6cef1e54cc218e`.
The catalogue checks the compressed download; tests also verify the expanded
weights. Compression uses `xz -6 --threads=1 --check=crc64 --stdout`.

Bundled model provenance is recorded in
[`anti-smudge.json`](../crates/neural/models/anti-smudge.json). The starting
network and checkpoint are from Yiguo Jiang, Xuhang Chen, Chi-Man Pun,
Shuqiang Wang and Wei Feng's
[MFDNet](https://github.com/Jiang-maomao/flare-removal), revision
`a9431498477ba8bf8f608d7f5fde1f841ced0a56`. The retrieved release materials did
not specify a weights licence; this model is not labelled with Schist's MIT
licence. Fine-tuning uses [FlareReal600](https://github.com/Zdafeng/FlareReal)
(publisher's dataset terms: CC BY-NC-SA 4.0) and synthetic small-light scatter.
The selected lineage contains 950 fine-tuning updates; details and validation
limits are below.

## Training data

Sources checked on 2026-09-25. Licenses below describe the publishers' stated
terms, not a determination of whether a particular training use is fair use or
whether trained weights are derivative works. Not committing source images does
not itself settle those questions. The loader accepts all these sources; it does
not impose a license-based restriction.

| Source | Data and relevance | Publisher's terms / download |
| --- | --- | --- |
| Google Research, *How to Train Neural Networks for Flare Removal* | 5,001 flare-only images: 2,001 lab/interpolated captures and 3,000 simulations. Useful scattering streaks; requires separate clean backgrounds. | [Official instructions and attribution](https://github.com/google-research/google-research/tree/master/flare_removal), flare images CC BY 4.0. `gs://gresearch/lens-flare` is publicly accessible. |
| Flare7K++ | 7,000 synthetic patterns plus 962 real captures through contaminated smartphone lenses. Scattering, streak and light-source annotations are particularly relevant. | [Official downloads](https://github.com/ykdai/Flare7K#data-download), [S-Lab noncommercial license](https://github.com/ykdai/Flare7K/blob/main/LICENSE). |
| FlareReal600 | 600 real aligned training pairs, 50 validation pairs, and 500 flare-only captures. Real paired data is useful for fine-tuning and evaluation. | [Official downloads](https://github.com/Zdafeng/FlareReal#data-download), [CC BY-NC-SA 4.0](https://github.com/Zdafeng/FlareReal/blob/main/LICENSE). Preserve the official split. |
| Open Images | Candidate clean scene backgrounds; curate night streets, lamps, illuminated windows and signs, plus flare-free negative examples. It is not already a flare-removal dataset. | [Official dataset licenses](https://storage.googleapis.com/openimages/web/factsfigures_v7.html#licenses). `tools/train/photos.py` checks the listed CC BY 2.0 license and records individual credits. Inspect selected images for existing flare and retain their attribution. |

Start with a small Google sample to test ingestion:

```sh
make fetch-anti-smudge-data ARGS='--out data/anti-smudge/google --limit 64'
# Omit --limit to fetch all 5,001 images. This is a much larger download.
```

The downloader verifies the published size and MD5, decodes each new image, and
records its source, license, attribution, version and SHA-256 in `credits.json`.
It does not require a Google account or the Cloud CLI. The `data/anti-smudge/` and
`runs/anti-smudge/` directories are ignored by git. Keep downloaded datasets,
checkpoints and source photos there, not in `crates/neural/models`.

For real aligned pairs, the dedicated downloader preserves the official 600/50
split and prepares matching `input`/`target` folders:

```sh
make fetch-anti-smudge-pairs ARGS='--out data/anti-smudge/flarereal600'
```

This downloads about 4.4 GB of archives from the official Google Drive links.
It verifies archive sizes and member CRCs, records original and processed image
hashes, and scales images to fit 1024×768 for CPU experiments. Use `--width` and
`--height` to choose a different bounding box. The source archives are retained.

A flare-only pattern is composited onto a clean scene **in linear light**, then
encoded to sRGB. The target retains the annotated light source. If the pattern
has no source annotation, the generator retains a small estimated light core;
that approximation is suitable for baseline experiments, not a substitute for
real paired validation. Twenty-five percent of training samples are clean
identity controls to penalize unwanted edits. Pairing happens before cropping,
so a streak can come from a light outside a crop.

Do not use the user's example as a clean target. Keep it as an independent visual
test. A single smudged image has no reference that can establish restoration
accuracy. No copy of the example is committed.

## Train and export

Python dependencies: PyTorch, NumPy, Pillow and ONNX. Use a virtual environment if
these are not already installed. Training can use CPU or CUDA. Runtime inference
inside Schist uses its existing Rust/tract runtime with native GPU contractions
when available; see [GPU inference](neural-gpu.md) for execution and fallback details.

For a developer preview through that same runtime, use:

```sh
make run-anti-smudge PROFILE=debug ARGS='runs/anti-smudge/paired.onnx input.png output.png 0.6'
```

The output path must not already exist; the source image is preserved.

For real aligned photos, prepare matching relative filenames:

```text
data/anti-smudge/paired/train/input/scene.png
                              target/scene.png
                      val/input/other-scene.png
                          target/other-scene.png
```

For synthesized pairs:

```text
data/anti-smudge/synthetic/train/clean/scene.jpg
                                 flares/pattern.png
                                 light_sources/pattern.png  # optional
                           val/clean/other-scene.jpg
                               flares/other-pattern.png
                               light_sources/other-pattern.png  # optional
```

Split by scene and capture session **before** creating crops or augmentations.
Keep neighboring video frames and patterns from the same aperture in the same
split. The script rejects byte-identical images across splits, even if renamed;
that check cannot detect all related scenes or near-duplicates. Do not train on
benchmark validation/test pairs. All images must be at least the patch size.

```sh
make train-anti-smudge ARGS='--mode synthetic \
  --train data/anti-smudge/synthetic/train \
  --val data/anti-smudge/synthetic/val \
  --out runs/anti-smudge/baseline.onnx --device cuda'

# Fine-tune on real aligned photos using the same channel count:
make train-anti-smudge ARGS='--mode paired \
  --train data/anti-smudge/paired/train \
  --val data/anti-smudge/paired/val \
  --initialize runs/anti-smudge/baseline.pt \
  --out runs/anti-smudge/paired.onnx --device cuda'
```

Use `--device cpu` when no GPU is available. Training writes an ONNX graph, a
weights-only-loadable `.pt` checkpoint and a `.json` report containing seeds,
source hashes and validation metrics. Select the best checkpoint using restored
image error plus clean-control error, including the initial checkpoint as a
candidate. Evaluation samples are fixed across epochs and use uniform crops.
Training favors affected crops, weights actual flare regions more heavily, and
keeps 25% identity controls. Reports include affected-region error as well as
PSNR and clean-control error; crop metrics do not replace full-image evaluation.

The default `--architecture residual` uses six dilated residual blocks with
short skip connections, leaky activations and a 161-pixel receptive field.
The original `--architecture dilated` stack remains available for old checkpoint
compatibility (191-pixel receptive field). Specify the same architecture and
channel count when fine-tuning. Short skips avoid the collapsing feature signal
observed in the initial 200-step synthetic smoke experiment. Very long streaks
can require a larger or multiscale model. Motion blur, defocus, atmospheric haze and reflection ghosts
are distinct failure cases. Clipped highlights contain missing information that
this model cannot reliably recover.

`--architecture pyramid` is a compact multiscale alternative. Its downsampling
stride is four, so training patches must be multiples of four. Export and tile
seam tests cover all three architectures. A run that selects step zero has not
beaten its initial checkpoint; do not present such an export as successful
training.

## MFDNet transfer training

The selected preview starts from the authors' [MFDNet checkpoint](https://github.com/Jiang-maomao/flare-removal)
and fine-tunes on FlareReal600. It is not a network trained from scratch by Schist.
Reference files are fetched separately and verified against a pinned revision
and SHA-256 hashes. Install `einops` alongside the other Python dependencies.

```sh
make train-anti-smudge-mfdnet ARGS='--fetch \
  --reference data/anti-smudge/mfdnet \
  --train data/anti-smudge/flarereal600/train \
  --val data/anti-smudge/flarereal600/val \
  --out runs/anti-smudge/mfdnet-real.onnx --device cpu'

# Resume with fresh crops and more emphasis on affected regions. Keep the
# previous checkpoint and export under a different name for comparison.
make train-anti-smudge-mfdnet ARGS='\
  --reference data/anti-smudge/mfdnet \
  --initialize runs/anti-smudge/mfdnet-real.pt \
  --train data/anti-smudge/flarereal600/train \
  --val data/anti-smudge/flarereal600/val \
  --out runs/anti-smudge/mfdnet-halo.onnx \
  --steps 400 --lr 7.5e-6 --seed 29 --focus-probability 0.75 \
  --light-start 0.55 --light-end 0.70 --device cpu'
```

Training uses equal restoration and clean-control losses, plus edge losses,
and exposure gains of 0.55–1.0 to include dimmer sources. The published MFDNet
inference pipeline recovers light sources after flare removal. This wrapper
uses a soft max-RGB mask from 0.5 to 0.65: a near-white cutoff missed the
underexposed lamp in the supplied photo. This recovery setting was adjusted
while inspecting that photo, so it is not an independently validated universal
threshold. The photo itself is never a clean training target.

`--light-start` and `--light-end` control this recovery mask. Raising them
restricts protection to the brighter light core, allowing more surrounding glow
to be removed; overly high values can erase dim sources. The halo refinement
recipe uses 0.55–0.70. `--focus-probability` changes how often the sampler chooses
the most affected of four random crops. Each degraded crop still has an equal
clean control, and the validation seed remains fixed when changing `--seed`.
Resuming loads network weights with a fresh optimizer; the reported step count
is for that run, not the combined training history.

The 300-step CPU run selected step 300. On 16 fixed held-out validation crops,
restoration MAE improved from the starting model's 0.07614 to 0.06773 and
clean-control MAE fell from 0.01214 to 0.00544. Input MAE was 0.08279; input and
restored PSNR were 17.59 and 18.90 dB. These are crop results, not evaluation of
all 50 validation photos or a guarantee on a new camera. The supplied photo is
a repeatedly inspected visual example, not a blind test.

The subsequent halo refinement ran 400 additional steps and selected step 250
of that run (550 training updates along the selected checkpoint's lineage).
With the tighter 0.55–0.70 recovery mask, the same 16 validation crops measured
restoration MAE 0.06360, clean-control MAE 0.00447, affected-region MAE 0.09760,
and restored PSNR 19.43 dB. Changing only the recovery mask on the previous
weights gave restoration MAE 0.06583 and clean-control MAE 0.00569; continuing
training improved both further. The final training step was not selected,
because its combined restoration and clean-control error was higher.

A broader check used one deterministic 256-pixel crop from each of the 50
official validation scenes (seed 20260925). Compared with the previous preview,
restoration MAE fell from 0.07568 to 0.07095 and clean-control MAE from 0.00767
to 0.00734. Error in a halo proxy region fell from 0.12881 to 0.11481. That region
is defined by positive input-minus-target RGB difference above 0.05 outside
target highlights of 0.65 or greater; it is not manually annotated halo data.
Bright target-region error increased slightly, from 0.05611 to 0.05695. These
remain crop comparisons, not full-scene accuracy measurements.

The export is approximately 27 MB. The default 1024-pixel input frame covers a
768-pixel working image in one context tile. For small distant lights, the
latest preview uses a 2048-pixel frame with a 1536-pixel working image. Keeping
one frame avoids seams caused by spatial attention statistics changing between
tiles. Only the predicted correction is enlarged back to the original photo.
The larger frame costs more CPU time and memory; weight size stays similar.

## Small distant lights

The next run mixes real pairs with controlled small-light examples. Each
synthetic target retains compact light cores; its degraded partner adds white,
warm or coloured scatter in linear light. These are approximate training
patterns, not a measured camera response. An extra loss penalizes positive
scatter outside bright target cores. The user's photo and marked crop are
visual evaluation examples only, never training targets.

```sh
make train-anti-smudge-mfdnet ARGS='\
  --reference data/anti-smudge/mfdnet \
  --initialize runs/anti-smudge/mfdnet-halo.pt \
  --train data/anti-smudge/flarereal600/train \
  --val data/anti-smudge/flarereal600/val \
  --out runs/anti-smudge/mfdnet-small.onnx \
  --steps 400 --lr 1e-5 --seed 41 --focus-probability .75 \
  --small-halo-probability .6 --halo-loss .35 \
  --light-start .65 --light-end .71 \
  --tile-size 2048 --working-max-side 1536 --halo-cleanup'
```

This run selected step 400, for 950 training updates along the selected
checkpoint's lineage. Selection includes real-pair and synthetic clean-control
errors, plus synthetic affected-region error. On the 50 real validation crops,
the neural stage's restoration MAE was 0.06682, clean-control MAE 0.00418, and
PSNR 18.85 dB, compared with the preceding preview's 0.07095, 0.00734, and
18.50 dB. Bright target-region MAE increased from 0.05695 to 0.05888. On the
16 controlled halo crops, PSNR was 37.16 dB versus the input's 35.38 dB;
global MAE alone remained worse than input because most pixels are unaffected.
Synthetic validation is not proof of camera restoration accuracy.

`--halo-cleanup` writes `schist.halo_cleanup=radial-v1` metadata. Schist then
applies a deterministic local correction after the network, before enlarging
the residual. This step is part of the filter; it is not baked into the ONNX
neural graph. It detects compact bright components, robustly fits a planar
background plus radial glow, and requires a substantial fit improvement and
positive scatter in at least six angular sectors. Border sources, broad bright
objects, singular fits and overlapping detections are skipped. Bright cores and
their immediate neighbourhood are protected. No photo coordinates or manual
masks are stored. Models without this metadata retain the previous behaviour.

The higher working resolution accounts for much of the improvement on the
small white and right-hand amber lamps. The local correction removes the
remaining broad glow around the middle amber lamp. Keep those contributions
separate from improvements due to additional neural training. This cleanup can
also remove real radial illumination; it is experimental, and its settings
were inspected on the supplied photo.

With cleanup enabled, the same 50-crop check measured restoration MAE 0.06682,
clean-control MAE 0.00467, and PSNR 18.85 dB. The extra cleanup slightly increases
clean-image changes compared with the neural stage alone, while the combined
result still changes clean controls less than the preceding preview. On the
supplied full photo it accepted one glow region automatically. The native
4032×3024 render took 308 seconds with about 2.1 GiB peak memory on the available
CPU; its output matched a separate PyTorch/NumPy implementation within 1/255,
including PNG quantization. More aggressive core feathering was rejected
because it shifted the middle lamp's edge toward green.

## Alternative starting point and calibration

The Flare7K++ Uformer is another starting point, stronger than the small
models trained briefly from scratch. Its authors provide the pretrained weights;
Schist does not claim to have trained that base network. The transfer tool pins
and verifies the architecture and checkpoint hashes, preserves the upstream
license, and exports the RGB restoration output for Schist:

```sh
# Requires einops as well as PyTorch, NumPy, Pillow and ONNX.
make transfer-anti-smudge ARGS='--fetch \
  --reference data/anti-smudge/flare7kpp \
  --out runs/anti-smudge/transfer.onnx'

# Optional: learn residual strength on real training pairs, with separate
# validation and equal penalties for restoration and clean-control errors.
make transfer-anti-smudge ARGS='\
  --reference data/anti-smudge/flare7kpp \
  --train data/anti-smudge/flarereal600/train \
  --val data/anti-smudge/flarereal600/val \
  --out runs/anti-smudge/calibrated.onnx'
```

Calibration trains one scalar; it does not retrain the frozen Uformer. If it
does not improve validation, the tool explicitly records `accepted: false` and
retains the published model. The report records source hashes, validation crops,
the learned and selected strengths, and the final model hash. The ONNX file is
about 86 MB and can be evaluated with `make run-anti-smudge`.

In the 2026-09-25 CPU experiments, the residual CNN and 1,600-step pyramid run
changed clean images too much. A 1,000-step output-layer fine-tune also lost to
the original Uformer on restoration MAE plus clean-control MAE. Finally, a
600-step residual calibration learned 1.17946 but was rejected on the same
criterion. That experiment therefore retained the **published checkpoint**.
On that calibration run's 16 fixed held-out crops, input/restored MAE was
0.09771/0.07344, with clean-control MAE 0.00640 (RGB values in 0–1). These are crop
validation results, not a claim that all 50 validation photos or the user's
camera have been restored successfully. Full-resolution visual inspection then
rejected this Uformer result because it dimmed the light center without removing
enough halo. The later MFDNet transfer run above is the selected preview.

## Use in Schist

Select **Filter → Neural Filters → Anti-Smudge** and preview the strength.
Native builds list the model as built in and ignore older externally installed
Anti-Smudge files. In the browser, first choose **Download** beside
**Anti-Smudge** in **Manage Models**; this fetches the bundled `.onnx.xz` asset
from the application's own host. Compressed bytes stay in memory for the tab's
lifetime, and the parsed model is cached after loading.

For development with other exported weights, use `make run-anti-smudge` as above;
the preview accepts both raw ONNX and XZ-compressed ONNX.

The input/output contract is float32 `[1, 3, H, W]`, RGB sRGB values in `[0,1]`,
one image output of the same shape. The bundled graph uses 2048-pixel frames
and 96 pixels of context on each side. An Anti-Smudge ONNX graph may declare
`schist.tile` as a multiple of 32 in 32–2048; the loader and inference use that
declared dimension. Older compact exports declare 384. Inspect results for
boundary artifacts when an export needs multiple tiles. Alpha and hidden
RGB in fully transparent pixels are preserved. A failed tile leaves the entire
filter buffer unchanged. Output is blended with the source using Strength.

A model may declare ONNX metadata `schist.restore_max_side` (32–2048 pixels).
The bundled model declares 1536; earlier exports used 768. For larger photos Schist area-averages a
working image, estimates restoration there, bilinearly enlarges only the
predicted correction, and adds it to the original pixels. This keeps original
fine detail and lets the model see broad scatter at its training scale. Models
without this metadata continue to run at full resolution. Both paths preserve
alpha and apply no partial result if inference fails.

## Validation

Run `make check-anti-smudge` for strict Clippy linting, synthesis, export parity, tile seam, failure
atomicity, archive integrity, embedded loading and filter tests.
`make check-anti-smudge-app` checks editor integration. Pixel tests use tiny
arithmetic ONNX fixtures; a separate test loads the actual compressed model
with deliberately invalid external model files to verify built-in loading.

Training PSNR alone is insufficient. Compare held-out real scenes at full
resolution for streak reduction, preserved lamp cores, text/window edges, color,
clean-image changes and tile seams. Keep capture devices and locations out of the
training set, and inspect the supplied example separately. Report hardware,
speed and memory on multi-megapixel photos. Preserve dataset credits and training
provenance with any weights proposed for distribution; resolve the relevant
permission or exception basis for that distribution separately.

## Runtime optimizations

Anti-Smudge uses the same Rust/embedded-PTX executor as background removal on
Linux/Windows when `SCHIST_NEURAL_CUDA=1` is set. All intermediates stay on the
GPU, with reused device workspaces, parallel spatial reductions and split-K
channel attention. Pooling, bicubic resize and the export's empty-ROI nearest resize are lowered
without host transfers. The system NVIDIA driver is the only dynamic dependency;
no inference runtime is bundled. Unsupported devices, insufficient VRAM and
execution failures retain the portable path. This remains opt-in pending NVIDIA
hardware parity and timing checks; see [CUDA validation](neural-cuda.md).

Portable CPU inference shares the bounded tensor worker pool: at most four
workers on Linux/Windows, two on Android, capped by available CPUs.
`SCHIST_NEURAL_CPU_THREADS` can override this (1–8). macOS Anti-Smudge also
uses at most four workers for its tiled CPU attention, alongside Accelerate
kernels. The Mac background-removal models retain their existing Core ML and
Accelerate scheduling; their default executor remains single-threaded.

The CPU plan also replaces supported 3×3 and 5×5 depthwise convolutions with
row-contiguous Rust kernels. They preserve filter-tap accumulation order and
use the existing bounded executor for independent output rows. Unsupported
layouts, strides, dilation and quantized operators retain tract. On macOS,
spatially banded Accelerate convolutions also cover the model's RGB output
layers, including valid convolutions after reflection padding; scratch remains
bounded to 16 MiB. These extra graph rewrites are scoped to Anti-Smudge.

The CPU attention pass recognizes the complete dense matrix-product, scalar
scale, softmax and value-product chain before general graph optimization. It
processes 32 queries at a time, retaining all keys and the original stable
softmax. A 512×512×512 score tensor (512 MiB) becomes two 32×512 score buffers
(128 KiB total per worker), plus a small value buffer. The Rust implementation
shares the bounded executor; macOS uses existing Accelerate BLAS and vForce
inside each block. Masks, shared intermediate outputs, non-scalar scaling,
unsupported layouts, types and dimensions retain the original operators. This
CPU-only rewrite leaves CUDA and GPU-partition graphs unchanged.

The restoration CPU plan also caches linear, nearest and bicubic resize plans,
processing independent output rows on the same bounded executor. Coordinates,
tap weights, skipped zero weights and axis/accumulation order retain tract's
behavior. Spatial reflection copies contiguous row interiors and fills only
their borders. The per-pixel channel maximum scans contiguous channel rows,
retaining tract's channel order, comparisons, signed zeros and non-finite
behavior. Other reduction
axes and padding geometries retain the original operators.

The atomic restoration driver fills the reflected model tensor directly and
copies only the needed output pixels. It avoids two 48 MiB RGB context buffers
for a 2048 tile. It still checks discarded context for non-finite results before
committing anything. Model weights, FP32 precision, 2048 context, 1536 working
resolution, light recovery and halo cleanup are unchanged.

Focused validation (all synthetic input; no photo artifacts retained):

```sh
make check-anti-smudge-runtime
make check-anti-smudge-cuda-graph
make check-neural-cuda
# macOS: full 2048 model against the previous CPU plan, with timings.
make check-anti-smudge-cpu-parity
# macOS: compare the latest kernels against the preceding accelerated plan.
make profile-anti-smudge-kernels
# Native: full model before/after tiled attention, with timings and parity.
make profile-anti-smudge-attention
# Native: compare image-row operations with the preceding tiled-attention plan.
make profile-anti-smudge-rows
# macOS: convolution band-size sweep with output parity checks.
make profile-anti-smudge-bands
# macOS: operator breakdown for the current full restoration graph.
make profile-anti-smudge
# NVIDIA: full graph CPU comparison and repeated GPU execution; no fallback allowed.
make check-neural-cuda-hardware-models
```

A paired release-build run on the local Apple Silicon Mac (2026-09-28), using
identical synthetic RGB input and the complete 2048 model, measured CPU inference
at **28.769 s before / 18.159 s after** (about 37% less time). CPU plan preparation
was 2.553 s / 1.592 s. Across all 12,582,912 output values, the maximum absolute
difference was **4.21e-6** on the unit scale. These are model-only, single-run
measurements, not an end-to-end camera-photo or NVIDIA timing claim.

The subsequent row-kernel/RGB-convolution pass was measured against that
already-accelerated CPU plan in one release executable. After one
warm-up pair, three alternating runs per plan gave medians of **18.420 s before /
15.931 s after** (about 14% less time). Warm-run ranges were 16.210–21.999 s before
and 14.544–19.641 s after; the optimized plan was faster in every pair despite
substantial timing variation. Across the same 12,582,912 values, maximum absolute
difference was **7.75e-7**. These remain local Mac, model-only measurements;
Linux/Windows/Android timing needs target hardware. The current graph uses 37
row-depthwise and 16 banded Accelerate convolutions. Regression checks cover
borders, narrow images, scalar/channel bias, batches, valid padding, rejected
geometry, and bit-identical serial/threaded row-kernel output.

Tiled attention plus the bounded Mac restoration pool was then compared with
the preceding production CPU plan (including the row/RGB kernels, using its
previous single-threaded executor). One warm-up pair followed by three
alternating runs measured **7.187 s before / 4.158 s after** in the same release
executable: about **42% less time**. Warm-run ranges were 7.037–7.218 s and
4.157–4.231 s respectively. All 14 intended attention blocks fused. Two complete
2048 synthetic inputs—the repeated RGB pattern and a spatial dark/bright
scene—produced identical final FP32 outputs in this check (maximum absolute
difference 0 across 25,165,824 values). This is a measured result for those
inputs, not a promise of bit-identical output for all inputs or architectures;
operator tests also compare the portable and Accelerate calculations against
tract across scaling, batches and block tails.

These timings measure Mac model inference, excluding loading and image cleanup.
The baseline also ran faster than in the earlier session, so compare the paired
before/after numbers within each run. Linux/Windows/Android timings remain
unmeasured. That pass's Mac operator profile placed all tiled attention at
0.507 s out of 4.189 s total.

The following image-row pass was compared with that tiled-attention plan, with
both plans using the same four-worker executor. One warm-up pair followed by
three alternating pairs measured **4.652 s before / 3.902 s after**, about
**16% less time**. Warm-run ranges were 4.602–4.671 s and 3.856–3.967 s. An initial
run had large swings (6–17 s) and was repeated before drawing a timing
conclusion. Both complete 2048 synthetic inputs again produced identical FP32
outputs (maximum difference 0 across 25,165,824 values).

All eight resizes, sixteen reflection pads and the RGB channel maximum use the
new row operations. A separate operator profile measured resizing at 0.047 s,
reflection at 0.136 s and the channel maximum at 0.002 s, out of 4.334 s total.
The convolution band sweep did not justify changing production band sizes.
Operator parity checks cover all six supported resize coordinate modes, linear,
nearest and cubic interpolation, sizes/scales, both nearest tie modes,
non-finite values, signed zeros, batch/plane boundaries and threaded execution.
These remain Mac CPU model-only timings; loading, image cleanup and other
platforms require separate measurements.

The full CUDA graph compiles to 2075 dispatches with **1758.4 MiB** of planned
resident tensor storage, excluding driver/context overhead. Source arithmetic
checks use synthetic operators, plus a small complete matting graph; only the
strict NVIDIA hardware suite can establish full restoration parity on the GPU.

## Core ML feasibility probe

Anti-Smudge can export as one complete Core ML MLProgram, but the September 28
Mac experiment did not justify changing the production backend. The app retains
the optimized Rust CPU path; no Anti-Smudge Core ML weights, Python dependency,
or additional runtime library are bundled by this probe.

`tools/train/anti_smudge_coreml.py` verifies the shipped ONNX checksums, folds
static shape calculations, and lowers the 71 unsupported operations: 32 L2
reductions, 32 normalization broadcasts, three flattens and four cubic resizes.
The cubic lowering preserves half-pixel sampling, coefficient -0.75 and clamped
edge taps. Export requires one complete partition with ONNX Runtime CPU fallback
disabled. ONNX Runtime is an offline converter here; predictions use the compiled
model through Apple's Core ML framework.

Four candidate graphs were measured on macOS 15.6.1 with coremltools 9.0 and
onnxruntime 1.30.0, using a deterministic `[1, 3, 2048, 2048]` FP32 input. Times
below exclude loading and image postprocessing. `CPU_AND_GPU` permits Core ML
to choose either device; it does not prove every operation ran on the GPU.

| Candidate | Compute units | Observed prediction times |
| --- | --- | --- |
| Original float32 graph | CPU + GPU | 16.90 s, then 94.01 s with severe memory pressure; stopped |
| Mixed precision, fused attention | CPU + GPU | 15.42 s |
| Fused attention padded to 32 channels | CPU + GPU | 15.78 s, 19.34 s |
| Mixed precision, 32-query attention blocks | CPU + GPU | 8.56 s first trial; separate repeat 9.48, 10.22, 11.89 s |
| Same blocked graph | CPU only | 12.43 s, 12.91 s |
| Same blocked graph | All devices, including Neural Engine | Loading exceeded the 45 s process limit; no inference timing |

The earlier paired Rust CPU median was 3.90 s. A later CPU operator-profile
control during this investigation took 7.41 s, so system load was variable and
these figures are not a controlled speedup ratio. None of the Core ML candidates
demonstrated a useful advantage. Full-image mixed-precision quality parity has
not been established; finite RGB outputs alone are insufficient to approve them.

The attention rewrite removes fourteen explicit 512×512×512 score tensors
(512 MiB each in float32). The blocked variant preserves score scaling and
softmax over every key. Fused variants compensate for SDPA's built-in scaling
and require macOS 15; the other variants target macOS 12. iOS performance is
unmeasured. Operator tests check cubic edge/fractional sampling and normalization
against ONNX Runtime, validate all three attention rewrites, and reject changed
topologies. Mixed variants preserve FP32 reductions, normalization divisions,
softmax, clipping, square roots and the L2 square operation.

Reproduce on a Mac with the Python dependencies above in `MATTING_PYTHON`:

```sh
make check-anti-smudge-coreml-export
make export-anti-smudge-coreml ARGS='--variant tiled --benchmark --runs 2'
make export-anti-smudge-coreml ARGS='--reuse-export --variant sdpa --benchmark'
```

All artifacts stay under `target/background-removal/anti-coreml`. Use a fresh
`--work` directory after changing exporter code or weights. Benchmarking runs in
a child process with a 45-second total load/prediction timeout (`--timeout` can
override it). This bounds stalled inference even when Core ML blocks the calling
Python thread. Temporary export and compiled model files from this investigation
were removed after validation.

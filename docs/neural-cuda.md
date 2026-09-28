# NVIDIA background and restoration inference

The Linux/Windows background and Anti-Smudge pipelines has an experimental CUDA executor in
`crates/neural/src/cuda`. Enable it with `SCHIST_NEURAL_CUDA=1` before launching
the app. It is opt-in until the device parity suite has been run on NVIDIA
hardware. Apple, Android and browser execution are unchanged.

The host executor, graph lowering, allocation planner and lifetime management
are Rust. The only library it opens is the installed NVIDIA driver:
`libcuda.so.1` on Linux or `nvcuda.dll` from Windows System32. No TensorRT,
cuDNN, cuBLAS, NVRTC, CUDA runtime, Python process or additional runtime DLL/SO
is distributed. Normal builds need only Cargo/Make. The small checked-in PTX
module is compiled offline from `kernels.cu`; there is no runtime C++ compiler.

The kernels require a 64-bit little-endian host and target compute capability
7.5 and later (including RTX 3060).
The driver must expose the CUDA 11-era versioned context/stream APIs used by
the loader. Unsupported devices/drivers simply retain portable inference.
`SCHIST_NEURAL_CUDA_DEVICE` selects a CUDA device ordinal (default 0).

## Execution and fallback

Tract infers concrete shapes and folds fixed metadata using exact integer
arithmetic. Rust lowers the supported typed operations before CPU packing.
A model is accepted only if its entire executable graph can remain on-device.
There are no intermediate host transfers and no implicit CPU operator islands.
Unsupported operators or layouts decline the graph before execution.

Float32 weights, activations and accumulation preserve the existing model
precision. Matrix products and dense convolutions use cooperative shared-memory
tiles; softmax uses a parallel row reduction. Long spatial reductions use
4096-element partials and a second device merge. Small attention matrices with
long K dimensions similarly split K to expose more blocks, avoiding a few blocks
serially processing an entire spatial plane. Strided copies, broadcasting,
sum/average pooling, leaky activations, reductions, padding, resizing and deformable sampling also stay on the GPU.
This first implementation does not use Tensor Cores, TF32 or quantization.
Transcendental functions use device float instructions and an erf approximation;
parity tests check numerical error rather than claiming bit-identical output.

Weights and execution parameters are uploaded once. Parameters and pointer
tables share one allocation per model, avoiding thousands of small driver
allocations. A last-use allocator reuses
intermediate buffers while protecting branches and all model outputs. The
executor uploads each input once and reads only the final outputs plus an error
flag. Shape metadata stays on the host. Runtime index checks set the error flag
rather than allowing out-of-range gathers.
Pageable uploads are synchronized before the nonblocking execution stream reads
them; returning from the host copy call alone does not guarantee DMA completion.

Startup's existing detached preload thread also prepares opted-in CUDA graphs
for already installed background models. Anti-Smudge loads on demand. It JIT-loads PTX through the driver and uploads
weights without running a dummy prediction. The driver's PTX cache avoids
recompilation on subsequent launches. Sessions share one serialized CUDA stream
and balance primary-context retain/release, restoring the caller's previous
context after use. Model eviction frees GPU allocations; background models retain the existing
five-minute desktop idle policy.

Each model checks its complete allocation requirement against available VRAM,
leaving 256 MiB plus a quarter of the remaining free space unused. CPU plans
remain available. Allocation/JIT failures retain the ordinary portable path;
launch, synchronization, invalid-index or nonfinite-output failures disable that
CUDA session, drain submitted work and rerun the input through portable inference.
Opted-in resident CUDA execution bypasses the CPU/partitioned-wgpu calibration.

## Validation and benchmarking

```sh
# Source-level arithmetic, graph lowering and workspace lifetime checks.
make check-neural-cuda
# Maintainer weights: both detectors, guide, opaque-core and detail refiners.
make check-neural-cuda NEURAL_CUDA_TEST_FILTER=all_background_models ARGS='--ignored --nocapture'
# Full 2048px restoration graph; no GPU required.
make check-anti-smudge-cuda-graph
# On a real NVIDIA machine: strictly requires CUDA; fallback cannot pass.
make check-neural-cuda-hardware
# Full-size background and Anti-Smudge parity, with installed weights; CPU references take longer.
make check-neural-cuda-hardware-models
# End-to-end portrait run. Output must not exist; only the final PNG is saved.
SCHIST_NEURAL_CUDA=1 make background-removal-example ARGS='input.jpg cuda-result.png 3 --preload'
# Comparison in a separate process, preserving the same source/model weights.
SCHIST_NEURAL_CUDA=0 make background-removal-example ARGS='input.jpg portable-result.png 3 --preload'
```

`Model::uses_cuda()` and the `CUDA resident graph` diagnostic distinguish CUDA
from fallback. Compare per-stage warm inference times separately from initial
loading/JIT time. Check alpha/hair edges against the existing output as well as
runtime. The local host arithmetic checker compiles the same scalar kernel
source and uses the same buffer-reuse plan, comparing against tract. It cannot
validate PTX execution, GPU barriers, driver ABI behavior or NVIDIA performance.
No RTX latency improvement is claimed until those hardware checks pass.

To regenerate the embedded module with LLVM 21 (maintainer-only):

```sh
make neural-cuda-ptx CUDA_CLANG=/path/to/llvm/bin/clang++
```

The Make target uses `-nocudainc -nocudalib`; the PTX contains no external
function dependencies. Review source and generated PTX together. CUDA Driver
API behavior and PTX portability are documented by
[NVIDIA](https://docs.nvidia.com/cuda/cuda-programming-guide/03-advanced/driver-api.html).

Reduction barriers follow the block synchronization requirements in NVIDIA's
[CUDA programming guide](https://docs.nvidia.com/cuda/archive/13.1.0/cuda-programming-guide/05-appendices/cpp-language-extensions.html).
Host checks reproduce the partial-sum order but cannot prove device synchronization.

These notices accompany the statically linked Apple Silicon inference runtime.

- `ort` / `ort-sys` 2.0.0-rc.13: https://github.com/pykeio/ort (MIT OR Apache-2.0).
- ONNX Runtime 1.28.0: https://github.com/microsoft/onnxruntime/tree/v1.28.0 (MIT; bundled third-party notices included).
- The crate downloads its checksum-pinned Core ML static archive during compilation. No runtime dylib is shipped or downloaded by the application.

The macOS app bundle includes this directory in its Resources.

The macOS compiled-model loader uses `objc2-core-ml` 0.3.2 (MIT OR Apache-2.0 OR Zlib), with `objc2`, `objc2-foundation` and `block2` (MIT). The upstream licensing overview is included as `objc2-LICENSE.md`. Core ML is linked from the system framework.

The lossless MIL weight writer follows Apple’s public Core ML protobuf and MILBlob storage formats: https://github.com/apple/coremltools/tree/main/mlmodel (BSD-3-Clause). The offline exporter uses coremltools 9.0; it is not included in the application. Model weight licenses remain in `models/licenses` and the original model cards.

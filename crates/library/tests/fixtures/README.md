These tiny ONNX graphs are synthetic test doubles, not trained face models.
They retain a real image input and emit deterministic detection/embedding tensors.
Tests exercise model loading, output decoding, crop coordinates, normalization,
replacement failure and instance isolation through the actual tract pipeline.

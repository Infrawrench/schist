"""Generate the small deterministic ONNX graphs used by GPU parity tests.

Run from any directory with NumPy and ONNX installed. Sin deliberately forces
partitioned execution, so the tests detect accidental CPU-only inference.
"""
from pathlib import Path

import numpy as np
import onnx
from onnx import TensorProto as T, helper as h, numpy_helper as n

OUT = Path(__file__).resolve().parents[1] / "crates/neural/tests/fixtures"


def tensor(name, values):
    return n.from_array(np.array(values, dtype=np.float32), name)


def save(name, nodes, initializers, shape, result, dtype=T.FLOAT, opset=13):
    graph = h.make_graph(
        nodes, name,
        [h.make_tensor_value_info("input", dtype, shape)],
        [h.make_tensor_value_info("output", T.FLOAT, result)],
        initializers,
    )
    model = h.make_model(graph, opset_imports=[h.make_opsetid("", opset)], ir_version=8)
    onnx.checker.check_model(model)
    onnx.save(model, OUT / (name + ".onnx"))


def weights(shape):
    return (np.arange(np.prod(shape), dtype=np.float32).reshape(shape) % 17 - 8) / 50


save(
    "gpu-prelu-dropout",
    [h.make_node("PRelu", ["input", "slope"], ["p"]),
     h.make_node("Dropout", ["p"], ["output"])],
    [tensor("slope", np.array([-.2, .1, 1.3]).reshape(3, 1, 1))],
    [1, 3, 4, 5], [1, 3, 4, 5], opset=11,
)
save(
    "gpu-partitioned-conv",
    [h.make_node("Sin", ["input"], ["s"]),
     h.make_node("Conv", ["s", "w", "bias"], ["c"], group=3,
                 strides=[2, 1], dilations=[2, 1], pads=[2, 1, 0, 3]),
     h.make_node("Conv", ["c", "w2", "bias2"], ["output"])],
    [tensor("w", weights((6, 1, 3, 3))), tensor("bias", np.arange(6) / 19),
     tensor("w2", weights((3, 6, 1, 1))), tensor("bias2", [-.1, .3, -.5])],
    [1, 3, 31, 27], [1, 3, 15, 29],
)
save(
    "gpu-partitioned-bands",
    [h.make_node("Sin", ["input"], ["s"]),
     h.make_node("Conv", ["s", "w", "bias"], ["output"],
                 dilations=[2, 1], pads=[2, 0, 2, 0])],
    [tensor("w", weights((6, 3, 3, 1))), tensor("bias", np.arange(6) / 19)],
    [1, 3, 684, 1027], [1, 6, 684, 1027],
)
save(
    "gpu-partitioned-einsum",
    [h.make_node("Sin", ["input"], ["s"]),
     h.make_node("Reshape", ["s", "shape"], ["r"]),
     h.make_node("MatMul", ["r", "w"], ["output"])],
    [n.from_array(np.array([2, 3, 10], dtype=np.int64), "shape"),
     tensor("w", weights((1, 10, 4)))],
    [1, 3, 4, 5], [2, 3, 4],
)
save(
    "gpu-partitioned-tokens",
    [h.make_node("Gather", ["table", "input"], ["emb"]),
     h.make_node("MatMul", ["emb", "w"], ["m"]),
     h.make_node("ReduceMean", ["m"], ["output"], axes=[1])],
    [tensor("table", weights((17, 8))), tensor("w", weights((8, 5)))],
    [1, 7], [1, 1, 5], dtype=T.INT64,
)

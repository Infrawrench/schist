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
    "gpu-partitioned-wide",
    [h.make_node("Sin", ["input"], ["s"]),
     h.make_node("Conv", ["s", "w", "bias"], ["output"],
                 strides=[2, 1], dilations=[2, 1], pads=[2, 1, 0, 3])],
    [tensor("w", weights((35, 3, 3, 3))), tensor("bias", np.arange(35) / 19)],
    [1, 3, 31, 27], [1, 35, 15, 29],
)
save(
    "gpu-partitioned-wide-bands",
    [h.make_node("Sin", ["input"], ["s"]),
     h.make_node("Conv", ["s", "w", "bias"], ["output"],
                 dilations=[2, 1], pads=[2, 0, 2, 0])],
    [tensor("w", weights((17, 3, 3, 1))), tensor("bias", np.arange(17) / 19)],
    [1, 3, 684, 1027], [1, 17, 684, 1027],
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

# Ragged tiles/reduction, transposed inputs/output, and multiple flattened row
# axes, including a compound reduction without host-side tensor packing.
for name, shape, weight_shape, equation, result, frame in [
    ("matrix-unit-axis", [1, 3, 17, 19], [1, 3, 19, 23], "abmk,abkn->bmn", [3, 17, 23], [1, 3, 17, 19]),
    ("matrix-tails", [3, 67, 71], [3, 71, 73], "bmk,bkn->bmn", [3, 67, 73], [1, 3, 67, 71]),
    ("matrix-transposed", [3, 17, 19], [3, 23, 19], "bmk,bnk->nbm", [23, 3, 17], [1, 3, 17, 19]),
    ("matrix-broadcast", [1, 51, 19], [2, 19, 23], "bmk,bkn->bmn", [2, 51, 23], [1, 3, 17, 19]),
    ("matrix-relative", [2, 3, 5, 17], [3, 7, 17], "bhwc,hkc->bhwk", [2, 3, 5, 7], [1, 3, 17, 10]),
    ("matrix-reductions", [3, 17, 19], [17, 19, 7], "mij,ijn->mn", [3, 7], [1, 3, 17, 19]),
]:
    save(
        "gpu-" + name,
        [h.make_node("Sin", ["input"], ["s"]),
         h.make_node("Reshape", ["s", "shape"], ["r"]),
         (h.make_node("MatMul", ["r", "w"], ["output"]) if name in ("matrix-broadcast", "matrix-tails")
          else h.make_node("Einsum", ["r", "w"], ["output"], equation=equation))],
        [n.from_array(np.array(shape, dtype=np.int64), "shape"), tensor("w", weights(weight_shape))],
        frame, result,
    )

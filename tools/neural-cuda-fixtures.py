"""Small CUDA parity fixtures for resize, slicing, padding and deformable sampling.

All pixels and weights are synthetic; no personal photos are stored.
"""
from pathlib import Path
import numpy as np
import onnx
from onnx import TensorProto as T, helper as h, numpy_helper as n

out = Path(__file__).resolve().parents[1] / "crates/neural/tests/fixtures"
def constant(name, values, dtype=np.float32):
    return n.from_array(np.array(values, dtype=dtype), name)

def save(name, shape, output_shape, nodes, constants, custom=False):
    graph = h.make_graph(nodes, name, [h.make_tensor_value_info("input", T.FLOAT, shape)],
                         [h.make_tensor_value_info("output", T.FLOAT, output_shape)], constants)
    model = h.make_model(graph, opset_imports=[h.make_opsetid("", 17)], ir_version=8)
    if not custom:
        onnx.checker.check_model(model)
    onnx.save(model, out / (name + ".onnx"))

for mode, coordinates, result in [("linear", "half_pixel", [2, 3, 7, 9]),
                                  ("linear", "align_corners", [2, 3, 3, 2]),
                                  ("nearest", "asymmetric", [2, 3, 7, 9])]:
    save("cuda-resize-" + coordinates, [2, 3, 4, 5], result,
         [h.make_node("Resize", ["input", "", "", "size"], ["output"],
                      mode=mode, coordinate_transformation_mode=coordinates, nearest_mode="floor")],
         [constant("size", result, np.int64)])

save("cuda-pad-slice", [2, 3, 4, 5], [2, 3, 4, 5],
     [h.make_node("Pad", ["input", "pads"], ["padded"], mode="reflect"),
      h.make_node("Slice", ["padded", "start", "end", "axes", "steps"], ["output"])],
     [constant("pads", [0, 0, 1, 2, 0, 0, 2, 1], np.int64),
      constant("start", [6, 7], np.int64), constant("end", [2, 2], np.int64),
      constant("axes", [2, 3], np.int64), constant("steps", [-1, -1], np.int64)])

save("cuda-integer", [2, 3, 4, 5], [2, 3, 4, 5],
     [h.make_node("Mul", ["input", "scale"], ["scaled"]),
      h.make_node("Floor", ["scaled"], ["rounded"]),
      h.make_node("Cast", ["rounded"], ["ints"], to=T.INT64),
      h.make_node("Add", ["ints", "two"], ["added"]),
      h.make_node("Mul", ["added", "nine"], ["multiplied"]),
      h.make_node("Div", ["multiplied", "four"], ["divided"]),
      h.make_node("Sub", ["divided", "two"], ["subtracted"]),
      h.make_node("Min", ["subtracted", "nine"], ["minimum"]),
      h.make_node("Max", ["minimum", "negative"], ["maximum"]),
      h.make_node("Cast", ["maximum"], ["output"], to=T.FLOAT)],
     [constant("scale", 7), constant("two", 2, np.int64),
      constant("nine", 9, np.int64), constant("four", 4, np.int64),
      constant("negative", -9, np.int64)])

save("cuda-dynamic-gather", [2, 3, 4, 5], [2, 3, 4, 5],
     [h.make_node("Mul", ["input", "scale"], ["scaled"]),
      h.make_node("Floor", ["scaled"], ["rounded"]),
      h.make_node("Cast", ["rounded"], ["indices"], to=T.INT64),
      h.make_node("Gather", ["data", "indices"], ["output"], axis=0)],
     [constant("scale", 7), constant("data", np.arange(16) / 17)])

for axis, shape in [(2, [2, 3, 17, 19]), (3, [2, 3, 4, 513])]:
    save("cuda-softmax-" + str(axis), shape, shape,
         [h.make_node("Softmax", ["input"], ["output"], axis=axis)], [])

save("cuda-grouped-conv", [2, 6, 9, 11], [2, 18, 4, 12],
     [h.make_node("Conv", ["input", "weights", "bias"], ["output"], group=2,
                  pads=[2, 1, 1, 2], strides=[2, 1], dilations=[2, 2])],
     [constant("weights", (np.arange(324).reshape(18, 3, 3, 2) % 23 - 11) / 31),
      constant("bias", (np.arange(18) - 9) / 17)])

# Batched/grouped deformable sampling, asymmetric kernel and negative indices.
# The custom op is precisely the typed fusion produced by deform_sample.rs.
data = [2, 2, 3, 4, 5]
sample = [2, 2, 3, 6, 2, 3]
kernel = [2, 3]
indices = np.arange(2*2*36*2).reshape(2, 2, 36, 2)
constants = []
for corner in range(4):
    value = indices.copy() + corner
    value[..., 0] = value[..., 0] % 4 - 4
    value[..., 1] = value[..., 1] % 5 - 5
    constants.append(constant(f"i{corner}", value, np.int64))
for corner in range(5):
    weights = (np.arange(144).reshape(2, 2, 1, 6, 2, 3) % 13 + corner) / 17
    constants.append(constant(f"w{corner}", weights))
save("cuda-deform", data, [2, 6, 4, 9],
     [h.make_node("SchistDeformSample", ["input"] + [f"i{i}" for i in range(4)] + [f"w{i}" for i in range(5)],
                  ["output"], data=data, sample=sample, kernel=kernel)], constants, custom=True)

# Restoration operators: empty ROI from PyTorch export, leaky activations,
# padded pooling and long, strided/multi-axis spatial statistics.
save("cuda-restore-resize", [2, 3, 4, 5], [2, 3, 8, 10],
     [h.make_node("Resize", ["input", "roi", "scale", ""], ["resized"],
                  mode="nearest", coordinate_transformation_mode="asymmetric", nearest_mode="floor"),
      h.make_node("LeakyRelu", ["resized"], ["output"], alpha=0.13)],
     [constant("roi", []), constant("scale", [1, 1, 2, 2])])
for include in [0, 1]:
    save(f"cuda-pool-{include}", [2, 3, 9, 11], [2, 3, 5, 4],
         [h.make_node("AveragePool", ["input"], ["output"], kernel_shape=[3, 4],
                      strides=[2, 3], pads=[1, 2, 1, 0], count_include_pad=include)], [])
save("cuda-pool-global", [2, 3, 9, 11], [2, 3, 1, 1],
     [h.make_node("AveragePool", ["input"], ["output"], kernel_shape=[9, 11])], [])
for kind in ["ReduceSum", "ReduceMax", "ReduceMin"]:
    save("cuda-" + kind, [2, 3, 65, 67], [1, 3, 1, 1],
         [h.make_node(kind, ["input", "axes"] if kind == "ReduceSum" else ["input"], ["output"],
                      **({} if kind == "ReduceSum" else {"axes": [0, 2, 3]}))],
         [constant("axes", [0, 2, 3], np.int64)] if kind == "ReduceSum" else [])

# Few output channels, long spatial reduction, non-multiple K and M/N tails.
save("cuda-split-matrix", [2, 3, 65, 67], [2, 3, 17],
     [h.make_node("Reshape", ["input", "shape"], ["flat"]),
      h.make_node("MatMul", ["flat", "weights"], ["output"])],
     [constant("shape", [2, 3, 4355], np.int64),
      constant("weights", (np.arange(4355*17).reshape(4355, 17) % 29 - 14) / 47)])

for coefficient in [-0.75, -0.5]:
    save(f"cuda-cubic{coefficient}", [2, 3, 4, 5], [2, 3, 7, 2],
         [h.make_node("Resize", ["input", "roi", "", "size"], ["output"],
                      mode="cubic", cubic_coeff_a=coefficient,
                      coordinate_transformation_mode="half_pixel")],
         [constant("roi", []), constant("size", [2, 3, 7, 2], np.int64)])

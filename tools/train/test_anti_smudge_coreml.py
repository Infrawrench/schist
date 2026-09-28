"""Numerical checks for unsupported ONNX operators lowered by the probe."""
import unittest

import numpy as np
import onnx
import onnxruntime as ort

from anti_smudge_coreml import lower_static


def tensor(name, values):
    return onnx.numpy_helper.from_array(np.asarray(values), name)


def model(nodes, inputs, output, constants=()):
    graph = onnx.helper.make_graph(nodes, 'lowering-test', [
        onnx.helper.make_tensor_value_info(name, onnx.TensorProto.FLOAT, shape)
        for name, shape in inputs.items()
    ], [onnx.helper.make_tensor_value_info('output', onnx.TensorProto.FLOAT, output)], constants)
    result = onnx.helper.make_model(graph, opset_imports=[onnx.helper.make_opsetid('', 11)])
    result.ir_version = 8
    onnx.checker.check_model(result)
    return result


def run(graph, inputs):
    options = ort.SessionOptions()
    options.intra_op_num_threads = options.inter_op_num_threads = 1
    options.log_severity_level = 3
    return ort.InferenceSession(graph.SerializeToString(), options,
                               providers=['CPUExecutionProvider']).run(None, inputs)[0]


class StaticLowering(unittest.TestCase):
    def test_cubic_matches_onnx_at_edges_and_fractional_coordinates(self):
        rng = np.random.default_rng(619)
        for height, width, oh, ow in [(5, 7, 10, 14), (8, 10, 4, 5),
                                      (1, 3, 2, 7), (8, 10, 7, 4)]:
            with self.subTest(size=(height, width, oh, ow)):
                shape, target = [1, 3, height, width], [1, 3, oh, ow]
                source = model([onnx.helper.make_node('Resize', ['input', 'roi', 'scales', 'sizes'],
                    ['output'], mode='cubic', coordinate_transformation_mode='half_pixel',
                    cubic_coeff_a=-.75)], {'input': shape}, target, [
                    tensor('roi', np.empty(0, np.float32)), tensor('scales', np.empty(0, np.float32)),
                    tensor('sizes', np.array(target, np.int64))])
                inputs = {'input': rng.normal(size=shape).astype(np.float32)}
                actual = lower_static(source)
                self.assertNotIn('Resize', [n.op_type for n in actual.graph.node])
                np.testing.assert_allclose(run(actual, inputs), run(source, inputs), atol=4e-6, rtol=4e-6)

    def test_l2_broadcast_and_flatten_match_with_zero_and_small_norms(self):
        nodes = [
            onnx.helper.make_node('ReduceL2', ['input'], ['norm'], axes=[2], keepdims=1),
            onnx.helper.make_node('Clip', ['norm', 'epsilon'], ['denominator']),
            onnx.helper.make_node('Expand', ['denominator', 'shape'], ['expanded']),
            onnx.helper.make_node('Div', ['input', 'expanded'], ['normalized']),
            onnx.helper.make_node('Flatten', ['normalized'], ['output'], axis=1),
        ]
        source = model(nodes, {'input': [2, 5, 3]}, [2, 15], [
            tensor('epsilon', np.array(1e-12, np.float32)), tensor('shape', np.array([2, 5, 3], np.int64))])
        inputs = {'input': np.linspace(-1, 1, 30, dtype=np.float32).reshape(2, 5, 3)}
        inputs['input'][0, 0] = 0
        inputs['input'][0, 1] *= 1e-7
        actual = lower_static(source)
        self.assertFalse({'ReduceL2', 'Expand', 'Flatten'} & {n.op_type for n in actual.graph.node})
        np.testing.assert_allclose(run(actual, inputs), run(source, inputs), atol=1e-7, rtol=1e-6)

    def test_rejects_expand_used_outside_denominator(self):
        source = model([
            onnx.helper.make_node('Expand', ['input', 'shape'], ['expanded']),
            onnx.helper.make_node('Add', ['other', 'expanded'], ['output']),
        ], {'input': [1, 1], 'other': [2, 3]}, [2, 3], [tensor('shape', np.array([2, 3], np.int64))])
        with self.assertRaises(AssertionError):
            lower_static(source)

    def test_rejects_expand_exposed_as_output(self):
        source = model([onnx.helper.make_node('Expand', ['input', 'shape'], ['output'])],
                       {'input': [1, 1]}, [2, 3], [tensor('shape', np.array([2, 3], np.int64))])
        with self.assertRaises(AssertionError):
            lower_static(source)


class AttentionLowering(unittest.TestCase):
    @staticmethod
    def program(count=14):
        import coremltools as ct
        from coremltools.converters.mil.mil import Builder as mb

        @mb.program(input_specs=[mb.TensorSpec(shape=(512, 512, 3)) for _ in range(3)],
                    opset_version=ct.target.macOS12)
        def graph(q, k, v):
            for _ in range(count):
                scores = mb.matmul(x=q, y=mb.transpose(x=k, perm=[0, 2, 1]))
                scores = mb.mul(x=scores, y=np.array([.5315447], np.float32))
                q = mb.matmul(x=mb.softmax(x=scores, axis=2), y=v)
            return q
        return graph

    def test_attention_variants_remove_large_score_tensors(self):
        from coreml_anti_smudge import attention
        for fused, padded in [(False, False), (True, False), (True, True)]:
            with self.subTest(fused=fused, padded=padded):
                program = self.program()
                attention(program, fused=fused, padded=padded)
                function = program.functions['main']
                self.assertEqual(function.outputs[0].shape, (512, 512, 3))
                self.assertFalse(any(v.shape == (512, 512, 512)
                                     for op in function.operations for v in op.outputs))
                kinds = [op.op_type for op in function.operations]
                self.assertEqual(kinds.count('scaled_dot_product_attention'), 14 if fused else 0)
                self.assertEqual(kinds.count('softmax'), 0 if fused else 14 * 16)
                program.validate()

    def test_changed_attention_topology_rejected(self):
        from coreml_anti_smudge import attention
        with self.assertRaisesRegex(AssertionError, 'pinned attention graph changed'):
            attention(self.program(count=13))


if __name__ == '__main__':
    unittest.main()

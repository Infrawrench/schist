"""Offline attention rewrites for the pinned Anti-Smudge Core ML graph."""
import numpy as np
import coremltools as ct
from coremltools.converters.mil.mil import Builder as mb
from coremltools.converters.mil.mil.passes.pass_registry import PASS_REGISTRY


def attention(program, fused=True, padded=False):
    function = program.functions['main']
    if fused:
        function.opset_version = ct.target.macOS15
    count = 0
    for softmax in list(function.operations):
        if softmax.op_type != 'softmax' or softmax.x.shape != (512, 512, 512):
            continue
        scale = softmax.x.op
        assert scale.op_type == 'mul' and scale.y.val.size == 1
        product = scale.x.op
        assert product.op_type == 'matmul'
        assert not product.transpose_x.val and not product.transpose_y.val
        assert softmax.axis.val == 2
        assert len(softmax.outputs[0].child_ops) == 1
        output = softmax.outputs[0].child_ops[0]
        assert output.op_type == 'matmul' and output.x is softmax.outputs[0]
        assert not output.transpose_x.val and not output.transpose_y.val
        assert list(product.outputs[0].child_ops) == [scale]
        assert list(scale.outputs[0].child_ops) == [softmax]
        q, k, v = product.x, product.y, output.y
        assert q.shape == v.shape == (512, 512, 3) and k.shape == (512, 3, 512)
        with function:
            if fused:
                # SDPA includes 1/sqrt(depth). Retain the learned temperature
                # by compensating on Q; parity checks bound rounding changes.
                depth = 32 if padded else 3
                q = mb.mul(x=q, y=np.float32(scale.y.val.item() * np.sqrt(depth)), before_op=output)
                k = mb.transpose(x=k, perm=[0, 2, 1], before_op=output)
                if padded:
                    q = mb.pad(x=q, pad=[0, depth - 3], before_op=output)
                    k = mb.pad(x=k, pad=[0, depth - 3], before_op=output)
                    v = mb.pad(x=v, pad=[0, depth - 3], before_op=output)
                result = mb.scaled_dot_product_attention(query=q, key=k, value=v,
                                                          before_op=output)
                if padded:
                    result = mb.slice_by_index(x=result, begin=[0, 0, 0], end=[512, 512, 3], before_op=output)
            else:
                chunks = []
                for first in range(0, 512, 32):
                    query = mb.slice_by_index(x=q, begin=[0, first, 0], end=[512, first + 32, 3], before_op=output)
                    scores = mb.matmul(x=query, y=k, before_op=output)
                    scores = mb.mul(x=scores, y=scale.y, before_op=output)
                    probabilities = mb.softmax(x=scores, axis=2, before_op=output)
                    chunks.append(mb.matmul(x=probabilities, y=v, before_op=output))
                result = mb.concat(values=chunks, axis=1, before_op=output)
            function.replace_uses_of_var_after_op(output, output.outputs[0], result)
            function.remove_ops([output, softmax, scale, product])
        count += 1
    assert count == 14, f'pinned attention graph changed: {count}'
    PASS_REGISTRY['common::dead_code_elimination'](program)
    print('Rewrote attention blocks:', count, 'fused' if fused else 'tiled', flush=True)

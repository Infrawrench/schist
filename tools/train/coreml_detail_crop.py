"""Restrict ViTMatte's local decoder to the center used by the native tiler.

The transformer still sees the complete 768px input. Propagating a 512px
output region backwards through the decoder retains every convolution and
bilinear interpolation neighbour that can contribute to that region.
"""
import numpy as np
import coremltools as ct
from coremltools.converters.mil.mil import Builder as mb
from coremltools.converters.mil.mil.passes.pass_registry import PASS_REGISTRY


def crop_detail_decoder(program):
    function = program.functions['main']
    original = function.outputs[0]
    assert original.shape == (1, 1, 768, 768)
    assert len(function.inputs) == 1
    assert next(iter(function.inputs.values())).shape == (1, 4, 768, 768)
    anchor = original.op
    regions = {}

    def decoder(value):
        op = value.op
        return op and ('_decoder_' in op.name or
                       any('_decoder_' in out.name for out in op.outputs))

    def convolution_bounds(op, region):
        assert op.pad_type.val == 'custom'
        sy, sx = op.strides.val
        dy, dx = op.dilations.val
        py, _, px, _ = op.pad.val
        kh, kw = op.weight.shape[-2:]
        top, left, bottom, right = region
        return tuple(map(int, (
            top * sy - py, left * sx - px,
            (bottom - 1) * sy - py + dy * (kh - 1) + 1,
            (right - 1) * sx - px + dx * (kw - 1) + 1,
        )))

    def clipped(value, region):
        top, left, bottom, right = region
        return (max(0, top), max(0, left),
                min(value.shape[-2], bottom), min(value.shape[-1], right))

    def dependencies(value, region):
        op = value.op
        if not decoder(value):
            return []
        if op.op_type in {'cast', 'relu', 'batch_norm', 'sigmoid'}:
            return [(op.x, region)]
        if op.op_type == 'concat':
            assert int(op.axis.val) == 1 and not op.interleave.val
            return [(item, region) for item in op.values]
        if op.op_type == 'conv':
            return [(op.x, clipped(op.x, convolution_bounds(op, region)))]
        if op.op_type == 'upsample_bilinear':
            assert not op.align_corners.val
            assert op.scale_factor_height.val == op.scale_factor_width.val == 2
            top, left, bottom, right = region
            # Half-pixel, 2x sampling. Include both input neighbours even when
            # the requested output begins or ends between input pixel centers.
            return [(op.x, clipped(op.x, ((top - 1) // 2, (left - 1) // 2,
                                          bottom // 2 + 1, right // 2 + 1)))]
        raise ValueError(f'unsupported detail decoder operation: {op.op_type} {op.name}')

    def need(value, region):
        if value in regions:
            old = regions[value]
            region = tuple(min(old[i], region[i]) if i < 2 else max(old[i], region[i])
                           for i in range(4))
            if old == region:
                return
        regions[value] = region
        for source, required in dependencies(value, region):
            need(source, required)

    need(original, (128, 128, 640, 640))
    # Fail on an upstream decoder topology change instead of silently treating
    # an unfamiliar branch as a full-resolution leaf and shipping a slowdown.
    assert len(regions) == 29, f'pinned detail decoder changed: {len(regions)} regions'
    rebuilt = {}

    def sliced(value, region, origin=(0, 0)):
        top, left, bottom, right = (region[0] - origin[0], region[1] - origin[1],
                                    region[2] - origin[0], region[3] - origin[1])
        if (top, left, bottom, right) == (0, 0, *value.shape[-2:]):
            return value
        assert 0 <= top < bottom <= value.shape[-2]
        assert 0 <= left < right <= value.shape[-1]
        return mb.slice_by_index(x=value, begin=[0, 0, top, left],
                                 end=[*value.shape[:2], bottom, right], before_op=anchor)

    def build(value, requested):
        region = regions[value]
        if value not in rebuilt:
            op = value.op
            if not decoder(value):
                output = sliced(value, region)
            else:
                args = dict(op.inputs)
                inputs = dependencies(value, region)
                if op.op_type == 'concat':
                    args['values'] = [build(item, extent) for item, extent in inputs]
                else:
                    args['x'] = build(*inputs[0])
                if op.op_type == 'conv':
                    raw = convolution_bounds(op, region)
                    bounded = inputs[0][1]
                    args['pad'] = [bounded[0] - raw[0], raw[2] - bounded[2],
                                   bounded[1] - raw[1], raw[3] - bounded[3]]
                output = getattr(mb, op.op_type)(**args, name='cropped_' + op.name,
                                                  before_op=anchor)
                if op.op_type == 'upsample_bilinear':
                    extent = inputs[0][1]
                    output = sliced(output, region, (extent[0] * 2, extent[1] * 2))
            assert output.shape[-2:] == (region[2] - region[0], region[3] - region[1])
            rebuilt[value] = output
        return sliced(rebuilt[value], requested, region[:2])

    with function:
        output = build(original, regions[original])
        original.name = 'unused_alpha'
        output.name = 'alpha'
        function.set_outputs([output])
        function.set_output_types([ct.TensorType(dtype=np.float32)])
    PASS_REGISTRY['common::dead_code_elimination'](program)

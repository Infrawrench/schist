#!/usr/bin/env python3
"""Offline Core ML feasibility probe; never installs models into the app.

Requires macOS with coremltools 9.0, onnx, and onnxruntime 1.30.0. Use
`make export-anti-smudge-coreml ARGS='--variant tiled --benchmark'`.
Mixed-precision variants are experiments, not quality-approved replacements.
"""
import argparse
import hashlib
import json
import lzma
from pathlib import Path
import shutil
import subprocess
import sys
import time
from collections import Counter

import onnx
import onnxruntime as ort
import numpy as np


def lower_static(model):
    """Express unsupported export operators with equivalent static primitives."""
    model = onnx.shape_inference.infer_shapes(model)
    graph = model.graph
    shapes = {v.name: [d.dim_value for d in v.type.tensor_type.shape.dim]
              for v in [*graph.input, *graph.value_info, *graph.output]}
    constants = {v.name: onnx.numpy_helper.to_array(v) for v in graph.initializer}
    users = {}
    for node in graph.node:
        for name in node.input:
            users.setdefault(name, []).append(node)
    replacements = {}
    nodes = []
    counts = Counter()
    opset = next(op.version for op in model.opset_import if op.domain in ('', 'ai.onnx'))

    def const(name, value):
        graph.initializer.append(onnx.numpy_helper.from_array(np.asarray(value), name))
        return name

    def node(kind, inputs, output, **attrs):
        nodes.append(onnx.helper.make_node(kind, inputs, [output], name=output, **attrs))
        return output

    for original in graph.node:
        original.input[:] = [replacements.get(v, v) for v in original.input]
        attrs = {a.name: onnx.helper.get_attribute_value(a) for a in original.attribute}
        output = original.output[0]
        prefix = output + '/schist_coreml'
        if original.op_type == 'ReduceL2':
            assert len(original.input) == 1 and set(attrs) <= {'axes', 'keepdims'}
            squared = node('Mul', [original.input[0]] * 2, prefix + '/square')
            if opset >= 13:
                axes = const(prefix + '/axes', np.array(attrs['axes'], dtype=np.int64))
                summed = node('ReduceSum', [squared, axes], prefix + '/sum', keepdims=attrs.get('keepdims', 1))
            else:
                summed = node('ReduceSum', [squared], prefix + '/sum', axes=attrs['axes'], keepdims=attrs.get('keepdims', 1))
            node('Sqrt', [summed], output)
        elif original.op_type == 'Expand':
            # Normalization denominators already broadcast in their consuming
            # divisions. Require that exact topology and static geometry.
            target = constants[original.input[1]].tolist()
            source = shapes[original.input[0]]
            assert len(source) == len(target) and all(a == b or a == 1 for a, b in zip(source, target))
            assert output not in {v.name for v in graph.output} and users.get(output)
            assert all(n.op_type == 'Div' and n.input[1] == output
                       and shapes[n.input[0]] == target for n in users[output])
            replacements[output] = original.input[0]
        elif original.op_type == 'Flatten':
            shape = const(prefix + '/shape', np.array(shapes[output], dtype=np.int64))
            node('Reshape', [original.input[0], shape], output)
        elif original.op_type == 'Resize' and attrs['mode'] == b'cubic':
            shape, target = shapes[original.input[0]], shapes[output]
            assert len(shape) == 4 and shape[:2] == target[:2]
            assert attrs['coordinate_transformation_mode'] == b'half_pixel'
            assert attrs['cubic_coeff_a'] == -.75 and not attrs.get('exclude_outside', 0)
            assert not attrs.get('antialias', 0)
            assert all(not name or constants[name].size == 0 for name in original.input[1:3])
            source = original.input[0]
            for axis in [2, 3]:
                scale = np.float32(target[axis] / shape[axis])
                positions = (np.arange(target[axis], dtype=np.float32) + np.float32(.5)) / scale - np.float32(.5)
                cell = np.ceil(positions) - np.float32(1)
                ratio = positions - cell
                terms = []
                for tap in range(4):
                    indices = np.clip(cell.astype(np.int64) - 1 + tap, 0, shape[axis] - 1)
                    x = np.abs(np.float32(tap - 1) - ratio)
                    a = np.float32(-.75)
                    weights = np.where(x <= 1,
                        (a + np.float32(2)) * x * x * x - (a + np.float32(3)) * x * x + np.float32(1),
                        np.where(x <= 2, a * x * x * x - np.float32(5) * a * x * x + np.float32(8) * a * x - np.float32(4) * a, np.float32(0)))
                    weight_shape = [1] * 4
                    weight_shape[axis] = target[axis]
                    name = f'{prefix}/{axis}/{tap}'
                    gathered = node('Gather', [source, const(name + '/indices', indices)], name + '/gather', axis=axis)
                    terms.append(node('Mul', [gathered, const(name + '/weights', weights.reshape(weight_shape))], name + '/weighted'))
                partial = terms[0]
                for tap in range(1, 4):
                    name = output if axis == 3 and tap == 3 else f'{prefix}/{axis}/sum{tap}'
                    partial = node('Add', [partial, terms[tap]], name)
                source = partial
        else:
            nodes.append(original)
            continue
        counts[original.op_type] += 1
    del graph.node[:]
    graph.node.extend(nodes)
    del graph.value_info[:]
    model = onnx.shape_inference.infer_shapes(model, strict_mode=True)
    onnx.checker.check_model(model)
    print('Static rewrites:', dict(counts), flush=True)
    return model


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--work', type=Path, default=Path('target/background-removal/anti-coreml'))
    parser.add_argument('--benchmark', action='store_true')
    parser.add_argument('--variant', choices=['float32', 'sdpa', 'sdpa-padded', 'tiled'], default='tiled')
    parser.add_argument('--reuse-export', action='store_true')
    parser.add_argument('--runs', type=int, default=1)
    parser.add_argument('--compute', choices=['gpu', 'cpu', 'all'], default='gpu')
    parser.add_argument('--predict', type=Path, help=argparse.SUPPRESS)
    parser.add_argument('--timeout', type=float, default=45, help='Total benchmark subprocess limit in seconds')
    args = parser.parse_args()
    if not 1 <= args.runs <= 10 or not 0 < args.timeout <= 300:
        parser.error('runs must be 1..10 and timeout must be >0 and <=300 seconds')
    if args.predict:
        import coremltools as ct
        start = time.monotonic()
        units = {'gpu': ct.ComputeUnit.CPU_AND_GPU, 'cpu': ct.ComputeUnit.CPU_ONLY, 'all': ct.ComputeUnit.ALL}
        predictor = ct.models.CompiledMLModel(str(args.predict), compute_units=units[args.compute],
            optimization_hints={'specializationStrategy': ct.SpecializationStrategy.FastPrediction})
        print('Direct Core ML load', time.monotonic() - start, flush=True)
        data = ((np.arange(3 * 2048 * 2048, dtype=np.int32) * 17 % 101).astype(np.float32) / np.float32(100)).reshape(1, 3, 2048, 2048)
        for i in range(args.runs):
            start = time.monotonic()
            result = predictor.predict({'input': data})['output']
            assert result.shape == data.shape and np.isfinite(result).all()
            print('Direct Core ML run', i, time.monotonic() - start, flush=True)
        return
    args.work.mkdir(parents=True, exist_ok=True)
    models = Path(__file__).resolve().parents[2] / 'crates/neural/models'
    metadata = json.loads((models / 'anti-smudge.json').read_text())
    compressed = (models / metadata['file']).read_bytes()
    assert hashlib.sha256(compressed).hexdigest() == metadata['compressed_sha256']
    raw = lzma.decompress(compressed)
    assert hashlib.sha256(raw).hexdigest() == metadata['model_sha256']
    identity = {
        'onnx_sha256': metadata['model_sha256'],
        'exporter_sha256': hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
        'attention_sha256': hashlib.sha256(Path(__file__).with_name('coreml_anti_smudge.py').read_bytes()).hexdigest(),
        'onnxruntime': ort.__version__,
    }
    record = args.work / 'export.json'
    if args.reuse_export:
        assert json.loads(record.read_text()) == identity, 'stale export; use a fresh --work directory'
        compile_variant(args)
        return
    assert not any(p.name != 'tmp' for p in args.work.iterdir()), 'use --reuse-export or a fresh --work directory'
    model = onnx.load_model_from_string(raw)
    onnx.helper.set_model_props(model, {
        **{p.key: p.value for p in model.metadata_props},
        'CACHE_KEY': hashlib.sha256(b'anti-smudge-coreml-probe-v1' + raw).hexdigest(),
    })
    source = args.work / 'source.onnx'
    onnx.save_model(model, source)
    options = ort.SessionOptions()
    options.graph_optimization_level = ort.GraphOptimizationLevel.ORT_ENABLE_BASIC
    options.intra_op_num_threads = 2
    options.inter_op_num_threads = 1
    options.add_session_config_entry('session.intra_op.allow_spinning', '0')
    options.add_session_config_entry('session.inter_op.allow_spinning', '0')
    options.optimized_model_filepath = str(args.work / 'optimized.onnx')
    ort.InferenceSession(str(source), sess_options=options, providers=['CPUExecutionProvider'])
    source = args.work / 'optimized.onnx'
    optimized = onnx.load(source)
    print('Static operators:', Counter(n.op_type for n in optimized.graph.node), flush=True)
    optimized = lower_static(optimized)
    onnx.helper.set_model_props(optimized, {'CACHE_KEY': hashlib.sha256(optimized.SerializeToString()).hexdigest()})
    source = args.work / 'lowered.onnx'
    onnx.save_model(optimized, source)
    options.optimized_model_filepath = ''
    options.add_session_config_entry('session.disable_cpu_ep_fallback', '1')
    options.log_severity_level = 2
    cache = args.work.resolve() / 'ort-cache'
    cache.mkdir(exist_ok=True)
    print('Exporting complete Core ML graph; CPU fallback disabled', flush=True)
    start = time.monotonic()
    session = ort.InferenceSession(str(source), sess_options=options, providers=[
        ('CoreMLExecutionProvider', {
            'ModelFormat': 'MLProgram',
            'MLComputeUnits': 'CPUAndGPU',
            'RequireStaticInputShapes': '1',
            'AllowLowPrecisionAccumulationOnGPU': '0',
            'ModelCacheDirectory': str(cache),
        }),
    ])
    print('Exported', time.monotonic() - start, session.get_providers(), flush=True)
    del session
    record.write_text(json.dumps(identity, indent=2) + '\n')
    compile_variant(args)


def compile_variant(args):
    import coremltools as ct
    from export_coreml import compact, FP32_OPERATIONS
    from coremltools.converters.mil.frontend.milproto.load import load
    from coreml_anti_smudge import attention

    cache = args.work.resolve() / 'ort-cache'
    candidates = list(cache.glob('*/*/model/Data/com.microsoft.OnnxRuntime/model.mlmodel'))
    assert len(candidates) == 1, f'expected one complete Core ML partition, got {len(candidates)}'
    source = ct.utils.load_spec(str(candidates[0]))
    assert [v.name for v in source.description.input] == ['input']
    assert [v.name for v in source.description.output] == ['output']
    base = args.work / 'float32'
    if not (base / 'report.json').exists():
        # Retry only this tool's incomplete build, never a completed export.
        if base.exists():
            shutil.rmtree(base)
        base.mkdir()
        _, report = compact(candidates[0], base)
        (base / 'report.json').write_text(json.dumps(report, indent=2) + '\n')
    work = args.work / args.variant
    compiled = work / 'model.mlmodelc'
    if not (work / 'report.json').exists():
        if work.exists():
            shutil.rmtree(work)
        work.mkdir()
        model = ct.models.MLModel(str(base / 'model.mlpackage'), skip_model_load=True)
        program = load(model.get_spec(), model.get_spec().specificationVersion, file_weights_dir=model.weights_dir)
        attention(program, fused=args.variant.startswith('sdpa'), padded=args.variant == 'sdpa-padded')
        precision = FP32_OPERATIONS | {'sqrt', 'clip'}
        model = ct.convert(program, source='milinternal', convert_to='mlprogram',
            compute_precision=ct.transform.FP16ComputePrecision(
                op_selector=lambda op: op.op_type not in precision and 'schist_coreml/square' not in op.name),
            minimum_deployment_target=ct.target.macOS15 if args.variant.startswith('sdpa') else ct.target.macOS12,
            skip_model_load=True)
        assert model.get_spec().description.input == source.description.input
        assert model.get_spec().description.output == source.description.output
        package = work / 'model.mlpackage'
        model.save(str(package))
        ct.models.utils.compile_model(str(package), destination_path=str(compiled))
        report = {'variant': args.variant, 'compute_precision': 'mixed_float16',
                  'float32_operations': sorted(precision), 'float32_l2_square': True,
                  'quality_approved': False, 'coremltools': ct.__version__}
        (work / 'report.json').write_text(json.dumps(report, indent=2) + '\n')
    if args.benchmark:
        # The parent stays outside Core ML's synchronous prediction call,
        # so a stalled prediction cannot prevent the timeout from firing.
        try:
            subprocess.run([sys.executable, __file__, '--predict', str(compiled),
                            '--runs', str(args.runs), '--compute', args.compute],
                           check=True, timeout=args.timeout)
        except subprocess.TimeoutExpired as error:
            raise SystemExit(f'Core ML benchmark stopped after {args.timeout:g}s (load + prediction)') from error


if __name__ == '__main__':
    main()

#!/usr/bin/env python3
"""Package the pinned background refiners for macOS (offline maintainer tool).

Run through `make export-background-coreml` on Apple Silicon with Xcode and
coremltools==9.0 installed in MATTING_PYTHON. The application uses Rust/Core ML;
Python, source ONNX, and the compiler are not included in the Mac bundle.
"""
import argparse
import hashlib
import io
import json
import lzma
from pathlib import Path
import shutil
import subprocess
import tarfile

import coremltools as ct
from coremltools.libmilstoragepython import _BlobStorageWriter
import numpy as np

IDS = ('detail-matting', 'subject-guide', 'matting')
VARIANTS = (*IDS, 'detail-matting-gpu')
FP32_OPERATIONS = frozenset({
    'layer_norm', 'reduce_mean', 'reduce_sum', 'softmax', 'pow',
    'rsqrt', 'real_div', 'sigmoid',
})


def sha(data):
    return hashlib.sha256(data).hexdigest()


def compact(source, destination, *, mixed=False):
    """Move inline float constants into standard MIL binary weight storage.

    ORT's transposed MatMul weights otherwise become huge hexadecimal text in
    model.mil. Storage compaction preserves the float32 values exactly; the
    optional GPU variant then applies a separate mixed-precision conversion.
    """
    spec = ct.utils.load_spec(str(source))
    weights = destination / 'weights'
    shutil.copytree(source.parent / 'weights', weights)
    writer = _BlobStorageWriter(str(weights / 'inline.bin'))
    count = size = 0
    for function in spec.mlProgram.functions.values():
        for block in function.block_specializations.values():
            for operation in block.operations:
                if operation.type != 'const':
                    continue
                value = operation.attributes['val']
                data = value.immediateValue.tensor.floats.values
                if len(data) < 10:
                    continue
                array = np.asarray(data, dtype=np.float32)
                offset = writer.write_float_data(array)
                count += 1
                size += array.nbytes
                value.ClearField('immediateValue')
                value.blobFileValue.fileName = '@model_path/weights/inline.bin'
                value.blobFileValue.offset = offset
            # ORT chooses the newest MIL dialect supported by the export Mac.
            # These fixed graphs only need Core ML 5 (macOS 12). Its Gather has
            # no validate_indices option; the newer export explicitly disables
            # it. Require that exact setting before removing the newer argument.
            constants = {op.outputs[0].name: op.attributes['val']
                         for op in block.operations if op.type == 'const'}
            for operation in block.operations:
                if 'validate_indices' in operation.inputs:
                    arguments = operation.inputs['validate_indices'].arguments
                    assert operation.type == 'gather' and len(arguments) == 1
                    value = constants[arguments[0].name]
                    assert list(value.immediateValue.tensor.bools.values) == [False]
                    del operation.inputs['validate_indices']
        old = function.opset
        assert len(function.block_specializations) == 1
        block = type(function.block_specializations[old])()
        block.CopyFrom(function.block_specializations[old])
        del function.block_specializations[old]
        function.opset = 'CoreML5'
        function.block_specializations['CoreML5'].CopyFrom(block)
    spec.specificationVersion = 6
    del writer
    package = destination / 'model.mlpackage'
    model = ct.models.MLModel(spec, weights_dir=str(weights), skip_model_load=True)
    if mixed:
        # Keep layer normalization, reductions, softmax and the final sigmoid
        # in float32. GPU-eligible arithmetic is otherwise lowered to float16;
        # the public image/trimap input and alpha output remain float32.
        from coremltools.converters.mil.frontend.milproto.load import load
        program = load(spec, spec.specificationVersion, file_weights_dir=str(weights))
        model = ct.convert(
            program, source='milinternal', convert_to='mlprogram',
            compute_precision=ct.transform.FP16ComputePrecision(
                op_selector=lambda operation: operation.op_type not in FP32_OPERATIONS),
            minimum_deployment_target=ct.target.macOS12, skip_model_load=True,
        )
        converted = model.get_spec()
        assert converted.description.input == spec.description.input
        assert converted.description.output == spec.description.output
        assert converted.specificationVersion == spec.specificationVersion
    model.save(str(package))
    compiled = destination / 'model.mlmodelc'
    ct.models.utils.compile_model(str(package), destination_path=str(compiled))
    return compiled, {'externalized_constants': count, 'externalized_bytes': size,
                      'specification_version': spec.specificationVersion,
                      'opsets': sorted({f.opset for f in spec.mlProgram.functions.values()}),
                      'compute_precision': 'mixed_float16' if mixed else 'float32',
                      'float32_operations': sorted(FP32_OPERATIONS) if mixed else None}


def pack(compiled, archive):
    files = []
    with lzma.open(archive, 'wb', preset=6) as stream:
        with tarfile.open(fileobj=stream, mode='w|', format=tarfile.USTAR_FORMAT) as tar:
            for path in sorted(compiled.rglob('*')):
                if not path.is_file():
                    continue
                assert not path.is_symlink()
                name = path.relative_to(compiled).as_posix()
                data = path.read_bytes()
                files.append((name, len(data), sha(data)))
                info = tarfile.TarInfo(name)
                info.size, info.mode, info.mtime = len(data), 0o644, 0
                tar.addfile(info, io.BytesIO(data))
    assert sum(size for _, size, _ in files) < 128 * 1024 * 1024
    assert archive.stat().st_size < 100 * 1024 * 1024, 'GitHub file size limit'
    return {'archive': archive.name, 'sha256': sha(archive.read_bytes()),
            'bytes': archive.stat().st_size, 'files': files}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--sources', type=Path, required=True, help='Rust export TSV')
    parser.add_argument('--work', type=Path, default=Path('target/background-removal/coreml-export'))
    parser.add_argument('--gpu-only', action='store_true',
                        help='Preserve verified float32 archives and rebuild only the GPU variant')
    args = parser.parse_args()
    sources = dict(line.split('\t', 1) for line in args.sources.read_text().splitlines())
    assert set(sources) == set(IDS)
    source_models = {}
    for id in IDS:
        paths = list(Path(sources[id]).glob('*/*/model/Data/com.microsoft.OnnxRuntime/model.mlmodel'))
        assert len(paths) == 1, f'{id}: expected one complete Core ML graph, got {len(paths)}'
        source_models[id] = paths[0]
    assert not args.work.exists(), 'Remove the previous export work directory before rebuilding'
    args.work.mkdir(parents=True)
    out = Path('crates/neural/models')
    manifest = {}
    if args.gpu_only:
        previous = json.loads((out / 'background-coreml.json').read_text())
        for id in IDS:
            record = previous[id]
            assert record['sha256'] == sha((out / record['archive']).read_bytes())
            assert record['source_onnx_xz_sha256'] == sha((out / (id + '.onnx.xz')).read_bytes())
            assert record.get('compute_precision', 'float32') == 'float32'
            manifest[id] = record
    updated = ('detail-matting-gpu',) if args.gpu_only else VARIANTS
    for id in updated:
        source_id = id.removesuffix('-gpu')
        work = args.work / id
        work.mkdir()
        compiled, report = compact(source_models[source_id], work, mixed=id.endswith('-gpu'))
        archive = work / (id + '.mlmodelc.tar.xz')
        record = pack(compiled, archive)
        record.update(report)
        record['source_onnx_xz_sha256'] = sha((out / (source_id + '.onnx.xz')).read_bytes())
        record['source_model'] = source_id
        record['target_arch'] = 'aarch64' if id.endswith('-gpu') else None
        record['coremltools'] = ct.__version__
        record['xcode'] = subprocess.check_output(['xcodebuild', '-version'], text=True).strip()
        manifest[id] = record
        print(id, record['bytes'], record['sha256'], flush=True)
    # Publish only after every model has compiled and packed successfully.
    for id in updated:
        filename = manifest[id]['archive']
        temporary = out / (filename + '.tmp')
        shutil.copyfile(args.work / id / filename, temporary)
        temporary.replace(out / filename)
    (out / 'background-coreml.json').write_text(json.dumps(manifest, indent=2) + '\n')
    write_assets(manifest)


def write_assets(manifest):
    rows = []
    for id in VARIANTS:
        record = manifest[id]
        files = ''.join(f'        ({json.dumps(n)}, {s}, {json.dumps(h)}),\n' for n, s, h in record['files'])
        cfg = '    #[cfg(target_arch = "aarch64")]\n' if id.endswith('-gpu') else ''
        rows.append(f'{cfg}    Asset {{ id: {json.dumps(id)}, archive: include_bytes!("../models/{record["archive"]}"), hash: {json.dumps(record["sha256"])}, files: &[\n{files}    ] }},')
    Path('crates/neural/src/native_coreml_assets.rs').write_text(
        '// Generated by make export-background-coreml. Source ONNX is excluded on macOS.\n'
        'const ASSETS: &[Asset] = &[\n' + '\n'.join(rows) + '\n];\n')


if __name__ == '__main__':
    main()

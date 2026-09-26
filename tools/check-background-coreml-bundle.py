#!/usr/bin/env python3
"""Verify the shipping Mac binary embeds only compiled background assets."""
import argparse
import hashlib
import json
import mmap
from pathlib import Path
import subprocess

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('binary', type=Path)
args = parser.parse_args()
models = Path(__file__).resolve().parents[1] / 'crates/neural/models'
manifest = json.loads((models / 'background-coreml.json').read_text())
with args.binary.open('rb') as file, mmap.mmap(file.fileno(), 0, access=mmap.ACCESS_READ) as binary:
    for id in ('detail-matting', 'subject-guide', 'matting'):
        record = manifest[id]
        archive = (models / record['archive']).read_bytes()
        assert hashlib.sha256(archive).hexdigest() == record['sha256'], id
        assert binary.find(archive) >= 0, f'{id}: compiled archive missing from executable'
        original = (models / (id + '.onnx.xz')).read_bytes()
        # Check multiple unique spans as well as the complete original, catching
        # an accidentally embedded copy even if link-time deduplication splits it.
        assert binary.find(original) < 0, f'{id}: original ONNX is embedded'
        for position in (256, len(original)//2, len(original)-4352):
            assert binary.find(original[position:position+4096]) < 0, f'{id}: ONNX payload remains'
        print(f'{id}: compiled archive present; ONNX absent')
links = subprocess.check_output(['otool', '-L', str(args.binary)], text=True)
assert 'onnxruntime' not in links.lower(), 'ONNX Runtime dylib linked'
print('No ONNX Runtime dylib dependency')

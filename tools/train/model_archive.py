"""Write reproducible compressed ONNX artifacts without leaving raw copies."""
import hashlib
import lzma
from pathlib import Path


def sidecar(path, suffix):
    path = Path(path)
    if not path.name.endswith(".onnx.xz"):
        raise ValueError("model output must end in .onnx.xz")
    return path.with_name(path.name[:-8] + suffix)


def write_archive(path, raw):
    path = Path(path)
    sidecar(path, ".json")
    compressed = lzma.compress(raw, format=lzma.FORMAT_XZ, preset=9)
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_name(path.name + ".tmp")
    temporary.write_bytes(compressed)
    temporary.replace(path)
    return {"onnx_sha256": hashlib.sha256(raw).hexdigest(), "bytes": len(raw),
            "compressed_sha256": hashlib.sha256(compressed).hexdigest(),
            "compressed_bytes": len(compressed),
            "compression": "XZ preset 9; expanded ONNX is byte-identical."}

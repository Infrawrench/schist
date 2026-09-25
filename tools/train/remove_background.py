#!/usr/bin/env python3
"""Run the same detector + tiled alpha refiner as Schist, writing a new RGBA PNG.

The source is never overwritten. Models are ONNX, so inference needs no torch.
Use --coarse-out to inspect the guided mask before alpha refinement.
"""

import argparse
import hashlib
import io
import lzma
from pathlib import Path

import numpy as np
from PIL import Image, ImageOps, ImageCms
import onnxruntime as ort

DETECTOR_SHA256 = "60920e99c45464f2ba57bee2ad08c919a52bbf852739e96947fbb4358c0d964a"
BIREFNET_SHA256 = "5600024376f572a557870a5eb0afb1e5961636bef4e1e22132025467d0f03333"
MATTING_DETECTOR_SHA256 = "273501048979b3012b544232234618819225745a13e316b21b4db439a2f28fe8"
DETECTOR_HASHES = {"isnet": DETECTOR_SHA256, "birefnet-lite": BIREFNET_SHA256,
                   "birefnet-matting": MATTING_DETECTOR_SHA256}
SRGB_PROFILE = ImageCms.ImageCmsProfile(ImageCms.createProfile("sRGB")).tobytes()


def load_rgba(path, max_edge=0):
    """Orient camera images and convert tagged colors to the model's sRGB space.

    Output files contain the sRGB profile, without copying GPS or camera EXIF.
    HEIC decoding is optional for users processing only PNG/JPEG files.
    """
    if Path(path).suffix.lower() in {".heic", ".heif", ".hif"}:
        try:
            import pillow_heif
        except ImportError as exc:
            raise RuntimeError("HEIC input requires pillow-heif; install matting-requirements.txt") from exc
        pillow_heif.register_heif_opener()
    if max_edge < 0:
        raise ValueError("max_edge must be nonnegative")
    with Image.open(path) as source:
        scale = min(1.0, max_edge / max(source.size)) if max_edge else 1.0
        if round(source.width * scale) * round(source.height * scale) > 16_777_216:
            raise ValueError("image exceeds the 16-megapixel processing limit")
        profile = source.info.get("icc_profile")
        rgba = ImageOps.exif_transpose(source).convert("RGBA")
        if profile:
            alpha = rgba.getchannel("A")
            rgba = ImageCms.profileToProfile(rgba.convert("RGB"),
                ImageCms.ImageCmsProfile(io.BytesIO(profile)), ImageCms.createProfile("sRGB"),
                outputMode="RGB").convert("RGBA")
            rgba.putalpha(alpha)
        rgba.info["original_size"] = rgba.size
        if max_edge:
            rgba.thumbnail((max_edge, max_edge), Image.Resampling.LANCZOS)
        return rgba


def session(path, low_memory=False):
    options = ort.SessionOptions()
    options.intra_op_num_threads = 4
    if low_memory:
        options.enable_cpu_mem_arena = False
        options.enable_mem_pattern = False
        options.graph_optimization_level = ort.GraphOptimizationLevel.ORT_ENABLE_BASIC
    if isinstance(path, bytes):
        model = path
    else:
        path = Path(path)
        model = lzma.decompress(path.read_bytes()) if path.suffix == ".xz" else str(path)
    return ort.InferenceSession(model, options, providers=["CPUExecutionProvider"])


def resize_linear(image, width, height, framing=False):
    """Pixel-centre bilinear sampling, including Schist's float32 coordinates.

    PIL applies an antialiasing kernel on reduction; the native model framing
    uses direct bilinear sampling, so use the same operation here.
    """
    h, w = image.shape[:2]
    x, y = np.arange(width, dtype=np.float32)+0.5, np.arange(height, dtype=np.float32)+0.5
    if framing:
        x = x/(np.float32(width)/np.float32(w))-0.5
        y = y/(np.float32(height)/np.float32(h))-0.5
    else:
        x = x*(np.float32(w)/np.float32(width))-0.5
        y = y*(np.float32(h)/np.float32(height))-0.5
    x, y = x.clip(0, w-1), y.clip(0, h-1)
    x0, y0 = x.astype(np.int32), y.astype(np.int32)
    x1, y1 = np.minimum(x0+1, w-1), np.minimum(y0+1, h-1)
    tx, ty = x-x0.astype(np.float32), y-y0.astype(np.float32)
    if image.ndim == 3:
        tx, ty = tx[None, :, None], ty[:, None, None]
    else:
        tx, ty = tx[None, :], ty[:, None]
    top = image[y0[:, None], x0[None, :]]*(1-tx)+image[y0[:, None], x1[None, :]]*tx
    bottom = image[y1[:, None], x0[None, :]]*(1-tx)+image[y1[:, None], x1[None, :]]*tx
    return top*(1-ty)+bottom*ty


def detect(model, rgb, kind="isnet", raw=False):
    h, w = rgb.shape[:2]
    peak = float(rgb.max())
    if kind == "isnet" and peak > 1e-4 and abs(peak-1) > 1e-3:
        rgb = rgb/peak
    small = (resize_antialiased(rgb, 1024, 1024) if kind.startswith("birefnet-")
             else resize_linear(rgb, 1024, 1024, framing=True))
    if kind.startswith("birefnet-") and kind in DETECTOR_HASHES:
        small = (small-np.array([.485, .456, .406], np.float32))/np.array([.229, .224, .225], np.float32)
    elif kind == "isnet":
        small = small-0.5
    else:
        raise ValueError(f"unknown detector kind: {kind}")
    small = small.transpose(2, 0, 1)
    alpha = model.run(None, {model.get_inputs()[0].name: small[None]})[0].squeeze()
    if not np.isfinite(alpha).all():
        raise ValueError("detector produced non-finite values")
    if kind.startswith("birefnet-"):
        alpha = 1/(1+np.exp(-alpha.clip(-80, 80)))
    alpha = alpha.clip(0, 1)
    return alpha if raw else resize_linear(alpha, w, h)


def resize_antialiased(rgb, width, height):
    """Float triangle filtering, with a wider footprint when reducing."""
    return np.stack([np.asarray(Image.fromarray(rgb[..., c]).resize(
        (width, height), Image.Resampling.BILINEAR)) for c in range(3)], axis=-1)


def refine(model, rgb, alpha):
    if model.get_inputs()[0].shape == [1, 4, 768, 768]:
        from detail_matting import refine_detail
        core_model = session(Path(__file__).resolve().parents[2] / "crates/neural/models/matting.onnx.xz")
        opaque_hint = refine(core_model, rgb, alpha)
        del core_model
        return refine_detail(model, rgb, alpha, opaque_hint)
    size, halo = 128, 16
    stride = size-2*halo
    h, w = alpha.shape
    result = np.empty_like(alpha)
    indices = np.arange(size)
    for y in range(0, h, stride):
        rows = (indices + y - halo).clip(0, h - 1)
        for x in range(0, w, stride):
            columns = (indices + x - halo).clip(0, w - 1)
            # Pad each tile, avoiding a full-resolution four-channel copy.
            coarse = alpha[rows[:, None], columns[None, :]]
            if coarse.max() <= 0.001 or coarse.min() >= 0.999:
                output = coarse
            else:
                tile = np.dstack([rgb[rows[:, None], columns[None, :]], coarse])
                tensor = tile.transpose(2, 0, 1)[None].copy()
                output = model.run(None, {model.get_inputs()[0].name: tensor})[0][0, 0]
            if not np.isfinite(output).all():
                raise ValueError("refiner produced non-finite values")
            bh, bw = min(stride, h-y), min(stride, w-x)
            result[y:y+bh, x:x+bw] = output[halo:halo+bh, halo:halo+bw]
    return result.clip(0, 1)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("image", type=Path)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--detector", type=Path)
    parser.add_argument("--detector-kind", choices=list(DETECTOR_HASHES), default="birefnet-matting")
    parser.add_argument("--refiner", type=Path, default=Path("crates/neural/models/detail-matting.onnx.xz"))
    parser.add_argument("--coarse-out", type=Path)
    parser.add_argument("--no-subject-guide", action="store_true", help="use generic salient-object detection only")
    parser.add_argument("--no-color-cleanup", action="store_true", help="export the matte with original RGB for comparison")
    parser.add_argument("--reference-detector", type=Path, default=Path("target/background-removal/birefnet-lite.onnx"))
    parser.add_argument("--no-detector-agreement", action="store_true", help="ablate the general-detector cross-check")
    args = parser.parse_args()
    if args.detector is None:
        args.detector = Path("target/background-removal") / {
            "birefnet-matting": "foreground-matting.onnx", "birefnet-lite": "birefnet-lite.onnx",
            "isnet": "isnet-general-use.onnx"}[args.detector_kind]
    for output in [args.out, args.coarse_out]:
        if output and (output.resolve() == args.image.resolve() or output.exists()):
            parser.error(f"refusing to overwrite {output}")
    if args.coarse_out and args.coarse_out.resolve() == args.out.resolve():
        parser.error("the two outputs must have different paths")
    expected_hash = DETECTOR_HASHES[args.detector_kind]
    if hashlib.sha256(args.detector.read_bytes()).hexdigest() != expected_hash:
        parser.error("detector hash does not match the pinned artifact")
    source = load_rgba(args.image)
    rgba = np.asarray(source).astype(np.float32)/255.0
    rgb = rgba[..., :3]*rgba[..., 3:4]+0.5*(1-rgba[..., 3:4])
    detector = session(args.detector, low_memory=args.detector_kind.startswith("birefnet-"))
    coarse = detect(detector, rgb, args.detector_kind)
    del detector
    reference = None
    if args.detector_kind == "birefnet-matting" and not args.no_subject_guide and not args.no_detector_agreement:
        if hashlib.sha256(args.reference_detector.read_bytes()).hexdigest() != BIREFNET_SHA256:
            parser.error("reference detector hash does not match the pinned artifact")
        general = session(args.reference_detector, low_memory=True)
        reference = detect(general, rgb, "birefnet-lite")
        del general
    if args.detector_kind.startswith("birefnet-") and not args.no_subject_guide:
        from subject_guidance import GUIDE_PATH, GUIDE_SHA256, subject_probability, guide_alpha, constrain_detail
        if hashlib.sha256(GUIDE_PATH.read_bytes()).hexdigest() != GUIDE_SHA256:
            parser.error("semantic guide hash does not match the pinned artifact")
        guide = session(GUIDE_PATH)
        probability = subject_probability(guide, rgb)
        if reference is not None:
            coarse = constrain_detail(coarse, reference, probability)
        coarse = guide_alpha(coarse, probability)
        del guide
    del reference
    matte = refine(session(args.refiner), rgb, coarse)
    from foreground_color import clean_foreground
    colors = clean_foreground(rgb, matte) if not args.no_color_cleanup else None
    for path, alpha in [(args.out, matte), (args.coarse_out, coarse)]:
        if path is None:
            continue
        result = np.asarray(source).copy()
        if path == args.out and colors is not None:
            active = (result[..., 3] == 255) & (alpha > 0) & (alpha < 1)
            result[..., :3][active] = (colors[active]*255).round().astype(np.uint8)
        result[..., 3] = (alpha*rgba[..., 3]*255).round().astype(np.uint8)
        path.parent.mkdir(parents=True, exist_ok=True)
        Image.fromarray(result).save(path, format="PNG", icc_profile=SRGB_PROFILE)
        print(path)


if __name__ == "__main__":
    main()

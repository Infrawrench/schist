#!/usr/bin/env python3
"""Local background-removal regression gallery; no photographs are uploaded.

Manifest: a JSON list of {"id": "case-name", "image": "/path/to/photo.heic"}.
Use a new output directory for each model revision. --long-edge 0 keeps the
original dimensions. The default 1600-pixel review is explicitly a preview.
"""

import argparse
import hashlib
import html
import json
from pathlib import Path
import re
import time
import gc
import shutil

import numpy as np
from PIL import Image

from remove_background import (BIREFNET_SHA256, DETECTOR_HASHES, SRGB_PROFILE,
                               detect, load_rgba, refine, session, resize_linear)


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def gallery(out, rows, before):
    cards = []
    links = [("Pixel crops", "qa/index.html"),
             ("Animal checks", "public-checks/index.html"),
             ("Audit results", "audit.json"),
             ("Native/Python comparison", "native-parity.json"),
             ("Integrity checks", "verification.json")]
    navigation = " · ".join(f'<a href="{url}">{title}</a>'
                             for title, url in links if (out / url).exists())
    timing_note = ("<p>Cached model predictions were reused where available. Times describe this processing pass, including cache loading.</p>"
                   if any(row.get("detector_cached") or row.get("reference_cached") or row.get("guide_cached") for row in rows) else "")
    for row in rows:
        case = row["id"]
        columns = [("Source", f"source/{case}.jpg")]
        if before:
            columns.append(("Previous model", f"before/{case}.png"))
        columns.append(("Updated model", f"cutouts/{case}.png"))
        images = ""
        for title, url in columns:
            preview = f"{'preview-before' if title == 'Previous model' else 'preview'}/{case}.png"
            thumbnail = preview if title != "Source" and (out / preview).exists() else url
            images += (f'<figure><figcaption>{title}</figcaption><a href="{url}">'
                       f'<img loading="lazy" src="{thumbnail}" alt="{title}"></a></figure>')
        note = f'<p>{html.escape(row["review_note"])}</p>' if row.get("review_note") else ""
        if row.get("alternative"):
            note += f'<p><a href="{html.escape(row["alternative"], quote=True)}">Alternate cutout from the reviewed preview detection</a></p>'
        resolution = " · reduced-size preview" if list(row["original_size"]) != [row["width"], row["height"]] else ""
        cached = " · detector prediction reused" if row.get("detector_cached") else ""
        cards.append(f'<article><h2>{html.escape(row["name"])}</h2><p>{row["width"]} × {row["height"]} '
                     f'pixels · {row["seconds"]:.1f}s · <a href="cutouts/{case}.png">PNG</a> · '
                     f'<a href="masks/{case}.png">Mask</a>{resolution}{cached}</p>{note}<section>{images}</section></article>')
    (out / "index.html").write_text('''<!doctype html><meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>Background removal review</title><style>
body{font:16px system-ui;margin:0;background:#f2f2f2;color:#171717}
header{position:sticky;top:0;background:#fff;padding:16px 24px;z-index:1;border-bottom:1px solid #ccc}
h1{font-size:22px;margin:0 0 8px}h2{font-size:18px}button{padding:8px 16px;margin-right:8px;cursor:pointer}
main{max-width:1400px;margin:auto;padding:24px}article{margin-bottom:40px}section{display:flex;gap:12px}
figure{flex:1;min-width:0;margin:0}figcaption{padding:8px;background:#fff}
figure a{display:block;background:var(--matte,#fff)}img{display:block;width:100%;height:auto}
a{color:#1760b4}p{line-height:1.5}@media(max-width:700px){section{flex-direction:column}}
</style><header><h1>Background removal review</h1>
<button data-bg="#fff">White</button><button data-bg="#000">Black</button><button data-bg="#7b8190">Gray</button>
<span>Click an image to inspect the PNG.</span></header><main>
<p>Processed locally. Originals are unchanged. Dimensions below are the actual exported dimensions;
1600-pixel results are review previews. This is a visual audit, not a full-image accuracy benchmark.</p>
''' + (f"<p>{navigation}</p>" if navigation else "") + timing_note + "\n".join(cards) + '''</main><script>
document.querySelectorAll('button[data-bg]').forEach(b=>b.onclick=()=>document.body.style.setProperty('--matte',b.dataset.bg));
</script>''')


def prepare_guides(args, cases):
    """Finish the small semantic model before loading the larger detector."""
    from subject_guidance import GUIDE_PATH, GUIDE_SHA256, subject_probability
    directory = args.out / "semantic"
    directory.mkdir(exist_ok=True)
    if args.guide_cache:
        provenance = json.loads((args.guide_cache / "provenance.json").read_text())
        for key, expected in {"model_sha256": GUIDE_SHA256, "manifest_sha256": digest(args.manifest),
                              "long_edge": args.long_edge}.items():
            if provenance.get(key) != expected:
                raise ValueError(f"cached semantic provenance differs: {key}")
        for case in cases:
            probability = np.load(args.guide_cache / (case["id"] + ".npy"))
            if probability.shape != (520, 520) or not np.isfinite(probability).all():
                raise ValueError(f"invalid cached semantic map: {case['id']}")
            np.save(directory / (case["id"] + ".npy"), probability)
        shutil.copyfile(args.guide_cache / "provenance.json", directory / "provenance.json")
        return {}
    model = session(GUIDE_PATH, low_memory=True)
    timings = {}
    for case in cases:
        start = time.time()
        image = load_rgba(case["image"], max_edge=args.long_edge)
        rgba = np.asarray(image)
        a = rgba[..., 3:4].astype(np.float32) / 255
        rgb = rgba[..., :3].astype(np.float32) / 255 * a + .5 * (1-a)
        probability = subject_probability(model, rgb)
        np.save(directory / (case["id"] + ".npy"), probability)
        timings[case["id"]] = time.time() - start
    (directory / "provenance.json").write_text(json.dumps({
        "model_sha256": GUIDE_SHA256, "manifest_sha256": digest(args.manifest),
        "long_edge": args.long_edge, "seconds": timings}, indent=2) + "\n")
    return timings


def prepare_references(args, cases):
    directory = args.out / "reference-detector"
    directory.mkdir(exist_ok=True)
    if args.reference_cache:
        provenance = json.loads((args.reference_cache / "run.json").read_text())
        for key, expected in {"detector_sha256": BIREFNET_SHA256, "manifest_sha256": digest(args.manifest),
                              "long_edge": args.long_edge}.items():
            if provenance.get(key) != expected:
                raise ValueError(f"cached reference provenance differs: {key}")
        saved = {row["id"]: row for row in provenance["cases"]}
        for case in cases:
            if saved[case["id"]]["source_sha256"] != digest(case["image"]):
                raise ValueError(f"cached source has changed: {case['id']}")
            raw = np.load(args.reference_cache / "raw-detector" / (case["id"] + ".npy"))
            if raw.shape != (1024, 1024) or not np.isfinite(raw).all():
                raise ValueError(f"invalid cached reference: {case['id']}")
            np.save(directory / (case["id"] + ".npy"), raw)
        return {}
    if digest(args.reference_detector) != BIREFNET_SHA256:
        raise ValueError("reference detector hash differs")
    model = session(args.reference_detector, low_memory=True)
    timings = {}
    for case in cases:
        start = time.time()
        image = load_rgba(case["image"], max_edge=args.long_edge)
        rgba = np.asarray(image).astype(np.float32)/255
        rgb = rgba[..., :3]*rgba[..., 3:4]+.5*(1-rgba[..., 3:4])
        raw = detect(model, rgb, "birefnet-lite", raw=True)
        np.save(directory / (case["id"] + ".npy"), raw)
        timings[case["id"]] = time.time()-start
        del image, rgba, rgb, raw
    return timings


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--manifest", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--detector", type=Path, required=True)
    parser.add_argument("--detector-kind", choices=list(DETECTOR_HASHES), required=True)
    parser.add_argument("--refiner", type=Path, default=Path("crates/neural/models/detail-matting.onnx.xz"))
    parser.add_argument("--before", type=Path, help="previous cutout directory, with matching case IDs")
    parser.add_argument("--long-edge", type=int, default=1600)
    parser.add_argument("--resume", action="store_true", help="continue an interrupted run after checking its provenance")
    parser.add_argument("--no-subject-guide", action="store_true", help="ablate the semantic subject guide")
    parser.add_argument("--no-color-cleanup", action="store_true", help="ablate foreground color estimation")
    parser.add_argument("--guide-cache", type=Path, help="reuse semantic maps with matching source/model provenance")
    parser.add_argument("--reference-cache", type=Path, help="reuse a matching general-detector review run")
    parser.add_argument("--reference-detector", type=Path, default=Path("target/background-removal/birefnet-lite.onnx"))
    parser.add_argument("--no-detector-agreement", action="store_true")
    parser.add_argument("--detector-cache", type=Path, help="reuse matching raw detector maps; infer missing cases normally")
    args = parser.parse_args()
    if (args.out.exists() and not args.resume) or args.long_edge < 0:
        parser.error("use a new output directory and a nonnegative long-edge limit")
    cases = json.loads(args.manifest.read_text())
    ids = [case["id"] for case in cases]
    if not cases or len(set(ids)) != len(ids) or any(not re.fullmatch(r"[A-Za-z0-9_-]+", case) for case in ids):
        parser.error("case IDs must be unique, nonempty and contain letters, digits, _ or -")
    expected = DETECTOR_HASHES[args.detector_kind]
    if digest(args.detector) != expected:
        parser.error("the detector does not match its pinned hash")
    for case in cases:
        if not Path(case["image"]).is_file():
            parser.error(f"missing input: {case['image']}")
        if args.before and not (args.before / (case["id"] + ".png")).is_file():
            parser.error(f"missing previous cutout: {case['id']}")
    from subject_guidance import GUIDE_PATH, GUIDE_SHA256, guide_alpha, constrain_detail
    use_guide = args.detector_kind.startswith("birefnet-") and not args.no_subject_guide
    use_reference = args.detector_kind == "birefnet-matting" and use_guide and not args.no_detector_agreement
    if use_guide and digest(GUIDE_PATH) != GUIDE_SHA256:
        parser.error("semantic guide hash does not match the pinned artifact")
    cached = {}
    if args.detector_cache:
        prior = json.loads((args.detector_cache / "run.json").read_text())
        for key, value in {"detector_sha256": expected, "manifest_sha256": digest(args.manifest),
                           "long_edge": args.long_edge}.items():
            if prior.get(key) != value:
                parser.error(f"detector cache provenance differs: {key}")
        cached = {row["id"]: row for row in prior["cases"]}
    report = {"pipeline_revision": 9, "detector": args.detector_kind, "detector_sha256": expected,
              "opaque_core_refiner_sha256": digest(Path(__file__).resolve().parents[2] / "crates/neural/models/matting.onnx.xz"),
              "foreground_color_cleanup": not args.no_color_cleanup,
              "reference_detector_sha256": BIREFNET_SHA256 if use_reference else None,
              "guide_sha256": GUIDE_SHA256 if use_guide else None,
              "refiner_sha256": digest(args.refiner), "long_edge": args.long_edge,
              "manifest_sha256": digest(args.manifest), "cases": []}
    if args.resume:
        old = json.loads((args.out / "run.json").read_text())
        if any(old.get(key) != value for key, value in report.items() if key != "cases"):
            parser.error("resume settings, models or manifest differ from the recorded run")
        if len(old["cases"]) > len(cases):
            parser.error("recorded run contains unexpected cases")
        for case, saved in zip(cases, old["cases"]):
            if case["id"] != saved["id"] or digest(case["image"]) != saved["source_sha256"]:
                parser.error("a completed source has changed")
            for folder in ["cutouts", "masks"]:
                with Image.open(args.out / folder / (case["id"] + ".png")) as check:
                    check.verify()
        report = old
    for directory in ["source", "cutouts", "masks", "before", "coarse", "raw-detector", "preview", "preview-before"]:
        (args.out / directory).mkdir(parents=True, exist_ok=args.resume)
    (args.out / "run.json").write_text(json.dumps(report, indent=2) + "\n")
    remaining = cases[len(report["cases"]):]
    if not remaining:
        gallery(args.out, report["cases"], args.before)
        return
    guide_timings = prepare_guides(args, remaining) if use_guide and remaining else {}
    gc.collect()
    reference_timings = prepare_references(args, remaining) if use_reference else {}
    gc.collect()
    for case in cases[len(report["cases"]):]:
        start = time.time()
        image = load_rgba(case["image"], max_edge=args.long_edge)
        original_size = image.info["original_size"]
        rgba = np.asarray(image)
        source_alpha = rgba[..., 3:4].astype(np.float32) / 255
        rgb = rgba[..., :3].astype(np.float32) / 255 * source_alpha + .5 * (1-source_alpha)
        detector_start = time.time()
        detector_cached = case["id"] in cached
        if detector_cached:
            if cached[case["id"]]["source_sha256"] != digest(case["image"]):
                raise ValueError(f"cached detector source has changed: {case['id']}")
            raw = np.load(args.detector_cache / "raw-detector" / (case["id"] + ".npy"))
            if raw.shape != (1024, 1024) or not np.isfinite(raw).all():
                raise ValueError("invalid cached detector output")
        else:
            detector = session(args.detector, low_memory=args.detector_kind.startswith("birefnet-"))
            raw = detect(detector, rgb, args.detector_kind, raw=True)
            # Match the native worker: release the large plan before allocating
            # full-resolution refinement/color buffers. Reloading costs less
            # than repeatedly paging its weights during the next stages.
            del detector
            gc.collect()
        detector_seconds = time.time() - detector_start
        np.save(args.out / "raw-detector" / (case["id"] + ".npy"), raw)
        coarse = resize_linear(raw, image.width, image.height)
        if use_guide:
            probability = np.load(args.out / "semantic" / (case["id"] + ".npy"))
            if use_reference:
                reference = resize_linear(np.load(args.out / "reference-detector" / (case["id"] + ".npy")), image.width, image.height)
                coarse = constrain_detail(coarse, reference, probability)
                del reference
            coarse = guide_alpha(coarse, probability)
        refiner_start = time.time()
        # The detail model is substantially larger than the legacy refiner.
        # Match the native worker: no detector and detail plans resident together.
        refiner = session(args.refiner, low_memory=True)
        alpha = refine(refiner, rgb, coarse)
        del refiner
        refiner_seconds = time.time() - refiner_start
        np.save(args.out / "coarse" / (case["id"] + ".npy"), coarse)
        del coarse
        result = rgba.copy()
        color_start = time.time()
        if not args.no_color_cleanup:
            from foreground_color import clean_foreground
            colors = clean_foreground(rgb, alpha)
            active = (rgba[..., 3] == 255) & (alpha > 0) & (alpha < 1)
            result[..., :3][active] = (colors[active]*255).round().astype(np.uint8)
            del colors, active
        color_seconds = time.time() - color_start
        result[..., 3] = (alpha * source_alpha[..., 0] * 255).round().astype(np.uint8)
        name = case["id"] + ".png"
        Image.fromarray(result).save(args.out / "cutouts" / name, icc_profile=SRGB_PROFILE)
        thumbnail = Image.fromarray(result)
        thumbnail.thumbnail((1000, 1000))
        thumbnail.save(args.out / "preview" / name, icc_profile=SRGB_PROFILE)
        Image.fromarray(result[..., 3]).save(args.out / "masks" / name)
        preview = image.convert("RGB")
        preview.thumbnail((1000, 1000))
        preview.save(args.out / "source" / (case["id"] + ".jpg"), quality=92, icc_profile=SRGB_PROFILE)
        if args.before:
            with Image.open(args.before / name) as old:
                old.convert("RGBA").save(args.out / "before" / name, icc_profile=SRGB_PROFILE)
                old.thumbnail((1000, 1000))
                old.save(args.out / "preview-before" / name, icc_profile=SRGB_PROFILE)
        row = {"id": case["id"], "name": case.get("name", Path(case["image"]).name),
               "source_sha256": digest(case["image"]), "original_size": original_size,
               "width": image.width, "height": image.height,
               "detector_seconds": detector_seconds, "refiner_seconds": refiner_seconds,
               "detector_cached": detector_cached,
               "reference_cached": bool(use_reference and args.reference_cache),
               "guide_cached": bool(use_guide and args.guide_cache),
               "color_seconds": color_seconds,
               "reference_seconds": reference_timings.get(case["id"], 0),
               "guide_seconds": guide_timings.get(case["id"], 0),
               "seconds": time.time()-start + guide_timings.get(case["id"], 0) + reference_timings.get(case["id"], 0)}
        report["cases"].append(row)
        (args.out / "run.json").write_text(json.dumps(report, indent=2) + "\n")
        gallery(args.out, report["cases"], args.before)
        print(json.dumps(row), flush=True)
        # Do not retain a previous full-resolution image while decoding the next.
        del image, rgba, source_alpha, rgb, raw, alpha, result, thumbnail, preview
        gc.collect()


if __name__ == "__main__":
    main()

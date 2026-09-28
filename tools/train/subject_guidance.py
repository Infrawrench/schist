"""Conservative semantic support for people and animals, separate from matting."""
from pathlib import Path
import numpy as np
from scipy.ndimage import maximum_filter, gaussian_filter, label

GUIDE_PATH = Path("crates/neural/models/subject-guide.onnx.xz")
GUIDE_SHA256 = "883dd1d4b4c7bdfc24c38ba516f2a299fa0fe1af355f22af4906a1627daa07fa"


def subject_probability(model, rgb):
    from remove_background import resize_antialiased
    small = resize_antialiased(rgb, 520, 520)
    tensor = ((small - np.array([.485, .456, .406], np.float32)) /
              np.array([.229, .224, .225], np.float32)).transpose(2, 0, 1)[None]
    probability = model.run(None, {model.get_inputs()[0].name: tensor})[0].squeeze()
    if probability.shape != (520, 520) or not np.isfinite(probability).all():
        raise ValueError("invalid semantic guide output")
    return probability.clip(0, 1)


def supported_components(mask, seeds, minimum=25):
    components, count = label(mask)
    hits = np.bincount(components[seeds].ravel(), minlength=count + 1)
    accepted = hits >= minimum
    accepted[0] = False
    return accepted[components].astype(np.float32)


def constrain_detail(coarse, reference, probability):
    """Admit new subject regions only with independent semantic support.

    The matting detector can recover clothing or reject signs, but sometimes
    grows into nearby furniture. Keep a small detail band around the general
    detector and a wider band around confidently recognized people/animals.
    With no recognizable subject, use the general detector for ordinary objects.
    """
    from remove_background import resize_linear
    if (reference.shape != coarse.shape or probability.shape != (520, 520)
            or not np.isfinite(reference).all() or not np.isfinite(coarse).all()
            or not np.isfinite(probability).all()):
        raise ValueError("invalid detector agreement input")
    if (probability > .8).mean() <= .003:
        return reference.copy()
    general = resize_linear(reference, 520, 520)
    agreement = maximum_filter((general > .1).astype(np.float32), size=7, mode="nearest")
    subject = maximum_filter((probability > .8).astype(np.float32), size=19, mode="nearest")
    gate = gaussian_filter(np.maximum(agreement, subject), sigma=1, truncate=3, mode="nearest")
    return coarse * resize_linear(gate, coarse.shape[1], coarse.shape[0])


def guide_alpha(coarse, probability):
    """Keep every supported subject, with room around hair/fur boundaries.

    This is not a largest-component filter. If no substantial person/animal
    is recognized, retain ordinary salient-object removal. The semantic model
    may remove alpha, but must never fill gaps between hair or body parts.
    """
    from remove_background import resize_linear
    if probability.shape != (520, 520) or not np.isfinite(probability).all():
        raise ValueError("invalid semantic guide probabilities")
    seeds = probability > .8
    if seeds.mean() <= .003:
        return coarse.copy()
    keep = supported_components(probability > .15, seeds)
    if not keep.any():
        return coarse.copy()
    support = maximum_filter(keep, size=19, mode="nearest")
    small_alpha = resize_linear(coarse, 520, 520)
    valid = supported_components(small_alpha * support > .1, seeds)
    gate = gaussian_filter(maximum_filter(valid, size=7, mode="nearest"),
                           sigma=1, truncate=3, mode="nearest") * support
    # Semantic context is weak at the frame edge (for example, a cropped cap).
    # Preserve detector extensions there only when connected to a supported
    # subject. Detached border objects and interior background objects stay out.
    full = supported_components(small_alpha > .1, seeds & (keep > 0))
    border = np.zeros_like(seeds)
    border[[0, -1], :] = True
    border[:, [0, -1]] = True
    extension = supported_components((full > 0) & (support == 0), border, minimum=1)
    gate = np.maximum(gate, gaussian_filter(
        maximum_filter(extension, size=7, mode="nearest"),
        sigma=1, truncate=3, mode="nearest"))
    height, width = coarse.shape
    return (coarse * resize_linear(gate, width, height)).clip(0, 1)

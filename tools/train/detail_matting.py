"""Native-resolution trimap matting with bounded, overlapping context windows."""
import numpy as np
from scipy.ndimage import maximum_filter, minimum_filter

SIDE, HALO, WINDOW, STRIDE = 768, 128, 512, 384
RADIUS = 4
CORE_RADIUS = 16
CORE_EXPANSION = 12


def trimap(alpha, opaque_hint=None):
    result = np.full(alpha.shape, .5, dtype=np.float32)
    result[minimum_filter(alpha, size=2*RADIUS+1, mode="nearest") > .98] = 1
    result[maximum_filter(alpha, size=2*RADIUS+1, mode="nearest") < .02] = 0
    if opaque_hint is not None:
        # The small refiner is useful for solid interiors, but its thin edge
        # predictions caused webbing. Only admit cores at least 33 pixels wide.
        core = minimum_filter(opaque_hint, size=2*CORE_RADIUS+1, mode="nearest") > .98
        # Recover the interior up to four pixels inside its old boundary.
        # Expansion is smaller than erosion, so this cannot fill old gaps or
        # invent foreground beyond the original opaque hint.
        core = maximum_filter(core, size=2*CORE_EXPANSION+1, mode="nearest")
        result[core & (result != 0)] = 1
    return result


def window_weights():
    x = np.arange(WINDOW, dtype=np.float32) + .5
    return np.minimum(np.minimum(x, WINDOW-x) / (WINDOW-STRIDE), 1)


def refine_detail(model, rgb, alpha, opaque_hint=None):
    if alpha.ndim != 2 or not alpha.size or alpha.size > 16_777_216 or rgb.shape != (*alpha.shape, 3):
        raise ValueError("invalid detail matting dimensions")
    if not np.isfinite(rgb).all() or not np.isfinite(alpha).all():
        raise ValueError("non-finite detail matting input")
    if opaque_hint is not None and (opaque_hint.shape != alpha.shape or not np.isfinite(opaque_hint).all()):
        raise ValueError("invalid opaque matting hint")
    h, w = alpha.shape
    tri = trimap(alpha, opaque_hint)
    result = np.zeros_like(alpha)
    weight_sum = np.zeros_like(alpha)
    ramp = window_weights()
    indices = np.arange(SIDE)
    for top in range(0, h, STRIDE):
        rows = (indices + top-HALO).clip(0, h-1)
        bh = min(WINDOW, h-top)
        for left in range(0, w, STRIDE):
            bw = min(WINDOW, w-left)
            dest = (slice(top, top+bh), slice(left, left+bw))
            known = tri[dest]
            if not np.any(known == .5):
                # Skip known areas without losing their contribution where
                # the next window overlaps them.
                output = known
            else:
                columns = (indices + left-HALO).clip(0, w-1)
                tile = np.dstack([rgb[rows[:, None], columns[None, :]].clip(0, 1),
                                 tri[rows[:, None], columns[None, :]]])
                tensor = tile.transpose(2, 0, 1)[None].copy()
                prediction = model.run(None, {model.get_inputs()[0].name: tensor})[0]
                if prediction.shape != (1, 1, SIDE, SIDE) or not np.isfinite(prediction).all():
                    raise ValueError("invalid detail matting output")
                output = prediction[0, 0, HALO:HALO+bh, HALO:HALO+bw].clip(0, 1)
            weight = ramp[:bh, None]*ramp[None, :bw]
            result[dest] += output*weight
            weight_sum[dest] += weight
    result /= weight_sum
    # Model predictions can be imperfect in a known region. Preserve the
    # confident interior and distant background independently of the model.
    return np.where(tri == .5, result, tri).astype(np.float32)

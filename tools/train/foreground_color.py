"""Remove background color mixed into partially transparent edge pixels.

Two-scale foreground/background estimation, following Marco Forte's
Approximate Fast Foreground Colour Estimation (ICIP 2021), also used by
BiRefNet's MIT-licensed image_proc.py. This changes color, never coverage.
Bounded overlapping chunks avoid full-photo temporary float planes.
"""

import numpy as np
from scipy.ndimage import uniform_filter

BLOCK = 256
RADII = (45, 3)
HALO = sum(RADII)


def fractional(alpha):
    # The editable mask is 8-bit. Tiny sigmoid tails round to fully hidden or
    # opaque coverage and must not recolor whole background/interior regions.
    coverage = alpha * np.float32(255)
    return (coverage > .5) & (coverage < 254.5)


def _blur(plane, radius):
    return uniform_filter(plane, size=2*radius+1, mode="nearest")


def _estimate(rgb, alpha):
    foreground, background = rgb.copy(), rgb.copy()
    for radius in RADII:
        mass = _blur(alpha, radius).clip(0, 1)
        for channel in range(3):
            f = _blur(foreground[..., channel]*alpha, radius)/np.maximum(mass, 1e-5)
            b = _blur(background[..., channel]*(1-alpha), radius)/np.maximum(1-mass, 1e-5)
            foreground[..., channel] = (f + alpha*(rgb[..., channel]-alpha*f-(1-alpha)*b)).clip(0, 1)
            background[..., channel] = b
    return foreground


def clean_foreground(rgb, alpha):
    """Return straight RGB; preserve fully opaque and fully hidden pixels."""
    if (alpha.ndim != 2 or rgb.shape != (*alpha.shape, 3)
            or not alpha.size or alpha.size > 16_777_216
            or not np.isfinite(rgb).all() or not np.isfinite(alpha).all()
            or (rgb < 0).any() or (rgb > 1).any()
            or (alpha < 0).any() or (alpha > 1).any()):
        raise ValueError("invalid foreground color input")
    result = rgb.copy()
    h, w = alpha.shape
    for top in range(0, h, BLOCK):
        for left in range(0, w, BLOCK):
            bottom, right = min(h, top+BLOCK), min(w, left+BLOCK)
            active = fractional(alpha[top:bottom, left:right])
            if not active.any():
                continue
            # Clip context at the actual image edge; each box filter replicates
            # its nearest sample there, exactly like the native implementation.
            y0, x0 = max(0, top-HALO), max(0, left-HALO)
            y1, x1 = min(h, bottom+HALO), min(w, right+HALO)
            estimated = _estimate(rgb[y0:y1, x0:x1], alpha[y0:y1, x0:x1])
            block = estimated[top-y0:bottom-y0, left-x0:right-x0]
            result[top:bottom, left:right][active] = block[active]
    return result

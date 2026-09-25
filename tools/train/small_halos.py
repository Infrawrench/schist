"""Controlled small-light scatter augmentation; targets retain the source cores.

This is a training approximation, not a measured camera point-spread function.
Only training/validation scene targets are used; never a user's smudged photo.
"""
import torch


def add_small_halos(scene, rng):
    """Return degraded/target CHW tensors with identical compact light sources."""
    _, height, width = scene.shape
    yy, xx = torch.meshgrid(torch.arange(height), torch.arange(width), indexing="ij")
    yy, xx = yy.to(scene), xx.to(scene)
    clean = scene.clamp(0, 1).pow(2.2)
    scatter = torch.zeros_like(clean)
    for _ in range(rng.randint(1, 4)):
        cx, cy = rng.uniform(8, width - 8), rng.uniform(8, height - 8)
        radius = rng.uniform(.65, 2.5)
        stretch = rng.uniform(.8, 1.3)
        distance = ((xx - cx) / stretch).square() + ((yy - cy) * stretch).square()
        core = torch.exp(-.5 * (distance / radius**2).pow(3))
        # White and warm sources, with occasional other lamp/signal colours.
        colours = [(1., .95, .8), (1., .55, .12), (1., .3, .03),
                   (1., .8, .3), (1., 1., 1.), (.1, .8, .7)]
        colour = torch.tensor(rng.choice(colours), dtype=scene.dtype,
                              device=scene.device)[:, None, None]
        intensity = rng.uniform(1.5, 7.)
        clean = clean + intensity * colour * core
        sigma = rng.uniform(2.5, 12.)
        halo = .75 * torch.exp(-distance / (2 * sigma**2))
        halo += .25 * torch.exp(-distance / (2 * (sigma * 2.2)**2))
        scatter = scatter + rng.uniform(.015, .15) * colour * halo
    target = clean.clamp(0, 1).pow(1 / 2.2)
    degraded = (clean + scatter).clamp(0, 1).pow(1 / 2.2)
    return degraded, target


class HaloValidation:
    def __init__(self, corpus):
        self.corpus = corpus

    def sample(self, rng, patch):
        _, scene = self.corpus.sample(rng, patch)
        bad, good = add_small_halos(scene, rng)
        gain = rng.uniform(.55, 1.)
        return bad * gain, good * gain


def halo_error(prediction, target, degraded):
    """Extra error outside bright cores where the input adds positive scatter."""
    halo = ((degraded - target).mean(1, keepdim=True) > .025)
    halo = halo & (target.amax(1, keepdim=True) < .65)
    weight = halo.to(prediction.dtype)
    return ((prediction - target).abs().mean(1, keepdim=True) * weight).sum() / weight.sum().clamp_min(1)

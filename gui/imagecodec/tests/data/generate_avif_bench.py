#!/usr/bin/env python3
"""Generate the AVIF decoding benchmark's inputs (`avifbench_*.avif`).

What a benchmark of a picture decoder decodes matters more than most: an AV1
decoder's time goes to entropy decoding and reconstruction, and a smooth
gradient or a flat colour codes to almost nothing, so it would time the
container and the colour conversion and call that decoding. These pictures
are made to have a photograph's statistics instead -- noise whose amplitude
falls as 1/f with frequency, which is how natural images' spectra fall, in
correlated colour, with hard-edged shapes and fine texture on top -- so that
the encoder spends its bits the way it does on a photograph.

Synthetic rather than a real photograph so that nothing here carries a
licence. Encoded by libavif 1.4.2 (aom) through `imagecodecs` 2026.8.16, at
quality 60 and speed 6, the middle of what websites serve.

  avifbench_8_420_640x480     a small picture: under THREADED_PIXELS, where a
                              decoder's threads would cost more than they save
  avifbench_8_420_1920x1080   the common web photograph -- and still just under
                              the threshold (2,073,600 < 2,097,152 pixels)
  avifbench_8_420_2560x1440   over it, so decoded threaded as shipped
  avifbench_10_444_1920x1080  deep colour at full chroma: the 16-bit paths

Run from this directory: python generate_avif_bench.py
"""

import sys

import imagecodecs
import numpy

SEED = 20260927


def pink(height, width, rng):
    """One channel of noise whose amplitude falls as 1/f, scaled to 0..1."""
    white = rng.standard_normal((height, width))
    fy = numpy.fft.fftfreq(height)[:, None]
    fx = numpy.fft.rfftfreq(width)[None, :]
    f = numpy.sqrt(fx * fx + fy * fy)
    f[0, 0] = 1.0
    spectrum = numpy.fft.rfft2(white) / f
    out = numpy.fft.irfft2(spectrum, s=(height, width))
    out -= out.min()
    return out / out.max()


def picture(height, width, depth):
    rng = numpy.random.default_rng(SEED + width)
    lum = pink(height, width, rng)
    # Colour that follows the light, as it does in a photograph, with its own
    # slower variation on top.
    warm = pink(height, width, rng)
    cool = pink(height, width, rng)
    r = 0.75 * lum + 0.25 * warm
    g = 0.85 * lum + 0.15 * (warm + cool) / 2
    b = 0.70 * lum + 0.30 * cool
    rgb = numpy.stack([r, g, b], axis=-1)
    # Hard edges: rectangles and discs of flat colour, as buildings and signs
    # give a photograph, and a band of fine stripes, as fabric or foliage.
    y, x = numpy.mgrid[0:height, 0:width]
    for _ in range(24):
        cx, cy = rng.integers(0, width), rng.integers(0, height)
        size = rng.integers(min(height, width) // 40, min(height, width) // 6)
        colour = rng.random(3)
        if rng.random() < 0.5:
            mask = (abs(x - cx) < size) & (abs(y - cy) < size // 2)
        else:
            mask = (x - cx) ** 2 + (y - cy) ** 2 < size * size
        rgb[mask] = 0.6 * colour + 0.4 * rgb[mask]
    band = (y > height * 0.7) & (y < height * 0.8)
    stripes = ((x + y // 3) % 7 < 3)[..., None] * 0.15
    rgb = numpy.where(band[..., None], rgb * 0.85 + stripes, rgb)
    top = (1 << depth) - 1
    dtype = numpy.uint8 if depth == 8 else numpy.uint16
    return numpy.clip(numpy.rint(rgb * top), 0, top).astype(dtype)


def main():
    for depth, chroma, width, height in [
        (8, "420", 640, 480),
        (8, "420", 1920, 1080),
        (8, "420", 2560, 1440),
        (10, "444", 1920, 1080),
    ]:
        data = imagecodecs.avif_encode(
            picture(height, width, depth),
            level=60,
            speed=6,
            bitspersample=depth,
            pixelformat=f"yuv{chroma}",
            numthreads=1,
        )
        name = f"avifbench_{depth}_{chroma}_{width}x{height}.avif"
        with open(name, "wb") as f:
            f.write(data)
        print(f"{name}: {len(data)} bytes")


if __name__ == "__main__":
    sys.exit(main())

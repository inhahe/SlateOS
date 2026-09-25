#!/usr/bin/env python3
"""Regenerate the GIF fixtures next to this script.

Two kinds, with two kinds of answer.

**Written by Pillow**, as GIFs are written in the wild: a photograph reduced
to 256 colours (interlaced, which is Pillow's default, and not), a four-colour
picture, and animations with transparency and each disposal. The answer is
Pillow's own decode, used only where Pillow and browsers agree -- every
animated frame here has a transparent index, which is the case in which
Pillow's disposal matches theirs (see `gif.rs`'s "Where decoders disagree").

**Written by hand** (`gifwriter.py`, validated by round-tripping 630 files of
every size, code size and clearing pattern through Pillow's decoder), for what
an ordinary encoder never produces: a dictionary that fills without a clear,
clears mid-stream, an image larger than its screen, one partly off it, no
colour table at all, indices past their table, GIF87a, loop counts. The answer
here is not any decoder's: it is the indices the file was made from, drawn by
`composite` below, which implements the rules browsers follow -- and which is
itself checked against Pillow on every fixture where the two should agree.

Each answer is `width height frames`, then per frame its delay in hundredths
of a second and `AARRGGBB` per pixel, fully transparent pixels written
`00000000` whatever colour they would have had.

Usage
-----

    python gui/imagecodec/tests/data/generate_gif.py

Requires Pillow.
"""

from __future__ import annotations

import io
import math
import pathlib
import random

from PIL import Image

from gifwriter import Frame, Gif

HERE = pathlib.Path(__file__).parent


def photo(w: int, h: int) -> Image.Image:
    """The same picture `generate_jpeg.py` draws: gradients, texture, a disc."""
    img = Image.new("RGB", (w, h))
    px = img.load()
    for y in range(h):
        for x in range(w):
            r = int(128 + 100 * math.sin(x / 7.0) * math.cos(y / 11.0))
            g = int((x * 255) / max(1, w - 1))
            b = int((y * 255) / max(1, h - 1))
            if (x + 2 * y) % 5 == 0:
                r, g, b = 255 - r, 255 - g, 255 - b
            cx, cy = w * 0.62, h * 0.4
            if (x - cx) ** 2 + (y - cy) ** 2 < (min(w, h) * 0.22) ** 2:
                r, g, b = 250, 40, 60
            px[x, y] = (max(0, min(255, r)), g, b)
    return img


# ---------------------------------------------------------------- answers

def write_answer(name: str, width: int, height: int, frames: list[tuple[int, list[int]]]) -> None:
    words = [str(width), str(height), str(len(frames))]
    for delay, pixels in frames:
        assert len(pixels) == width * height, (name, len(pixels), width * height)
        words.append(str(delay))
        words.extend("00000000" if p >> 24 == 0 else f"{p:08X}" for p in pixels)
    with open(HERE / f"{name}.txt", "w", encoding="ascii", newline="\n") as out:
        out.write(" ".join(words) + "\n")


def pillow_frames(data: bytes) -> tuple[int, int, list[tuple[int, list[int]]]]:
    """Pillow's decode of every frame, as `0xAARRGGBB`."""
    im = Image.open(io.BytesIO(data))
    frames = []
    for i in range(getattr(im, "n_frames", 1)):
        im.seek(i)
        rgba = im.convert("RGBA")
        raw = rgba.tobytes()
        pixels = [
            (raw[k + 3] << 24) | (raw[k] << 16) | (raw[k + 1] << 8) | raw[k + 2]
            for k in range(0, len(raw), 4)
        ]
        delay = im.info.get("duration", 0) // 10
        frames.append((delay, pixels))
    w, h = im.size
    return w, h, frames


def argb(colours, index: int) -> int:
    if colours is None:
        return 0xFF000000 | (index << 16) | (index << 8) | index
    if index < len(colours):
        r, g, b = colours[index]
        return 0xFF000000 | (r << 16) | (g << 8) | b
    return 0xFF000000


def padded(colours):
    """The table as the file carries it: padded to a power of two with black."""
    if colours is None:
        return None
    n = 2
    while n < len(colours):
        n *= 2
    return list(colours) + [(0, 0, 0)] * (n - len(colours))


def composite(gif: Gif, drawn: list[int] | None = None) -> tuple[int, int, list[tuple[int, list[int]]]]:
    """Every frame of `gif` as a browser shows it.

    The canvas starts transparent and is the logical screen enlarged to hold
    the first image; later images are clipped to it. Disposal 2 clears to
    transparent, 3 restores what was under the image -- on the first frame,
    the empty canvas. `drawn[i]`, if given, is how many of frame `i`'s indices
    (in file order) arrived, for a file cut short.
    """
    width, height = gif.width, gif.height
    if gif.frames:
        first = gif.frames[0]
        width = max(width, first.left + first.width)
        height = max(height, first.top + first.height)
    canvas = [0] * (width * height)
    out = []
    pending = None
    for n, f in enumerate(gif.frames):
        if pending is not None:
            kind, rect, saved = pending
            x0, y0, x1, y1 = rect
            for y in range(y0, y1):
                for x in range(x0, x1):
                    canvas[y * width + x] = 0 if kind == 2 else saved[(y - y0) * (x1 - x0) + (x - x0)]
            pending = None
        x0, y0 = min(f.left, width), min(f.top, height)
        x1 = max(x0, min(f.left + f.width, width))
        y1 = max(y0, min(f.top + f.height, height))
        saved = [canvas[y * width + x] for y in range(y0, y1) for x in range(x0, x1)]
        colours = padded(f.palette if f.palette is not None else gif.palette)
        rows = list(range(f.height))
        if f.interlace:
            rows = [r for start, step in ((0, 8), (4, 8), (2, 4), (1, 2)) for r in range(start, f.height, step)]
        limit = len(f.indices) if drawn is None else drawn[n]
        for k in range(min(limit, len(f.indices))):
            sent_row, x = divmod(k, f.width)
            y = f.top + rows[sent_row]
            cx = f.left + x
            if cx >= width or y >= height:
                continue
            index = f.indices[rows[sent_row] * f.width + x]
            if index == f.transparent:
                continue
            canvas[y * width + cx] = argb(colours, index)
        out.append((f.delay, list(canvas)))
        if f.disposal in (2, 3):
            pending = (f.disposal, (x0, y0, x1, y1), saved)
    return width, height, out


def same_as_pillow(data: bytes, answer) -> bool:
    """Whether Pillow decodes `data` to `answer`, transparent pixels alike."""
    w, h, frames = pillow_frames(data)
    aw, ah, aframes = answer
    if (w, h) != (aw, ah) or len(frames) != len(aframes):
        return False
    for (_, got), (_, want) in zip(frames, aframes):
        norm = lambda ps: [0 if p >> 24 == 0 else p for p in ps]
        if norm(got) != norm(want):
            return False
    return True


# ---------------------------------------------------------------- fixtures

def pillow_written() -> None:
    small = photo(97, 61)
    quantised = small.quantize(colors=256, dither=Image.Dither.FLOYDSTEINBERG)
    for name, kw in [("gif_photo", {}), ("gif_photo_flat", {"interlace": False})]:
        buf = io.BytesIO()
        quantised.save(buf, "GIF", **kw)
        (HERE / f"{name}.gif").write_bytes(buf.getvalue())
        write_answer(name, *pillow_frames(buf.getvalue()))

    four = small.quantize(colors=4, dither=Image.Dither.NONE)
    buf = io.BytesIO()
    four.save(buf, "GIF")
    (HERE / "gif_four.gif").write_bytes(buf.getvalue())
    write_answer("gif_four", *pillow_frames(buf.getvalue()))

    # A disc crossing a transparent field, in a colour of its own each frame.
    frames = []
    for i in range(6):
        im = Image.new("RGBA", (40, 24), (0, 0, 0, 0))
        for y in range(24):
            for x in range(40):
                if (x - 6 - 5 * i) ** 2 + (y - 12) ** 2 < 30:
                    im.putpixel((x, y), (250, 40 + 35 * i, 60 + 20 * i, 255))
                elif (x + y + i) % 7 == 0:
                    im.putpixel((x, y), (20, 20 + 30 * i, 200, 255))
        frames.append(im)
    for name, disposal in [
        ("gif_anim_clear", 2),
        ("gif_anim_keep", 1),
        # Never 3 on the first frame: there Pillow keeps the frame where
        # browsers put back the empty canvas.
        ("gif_anim_restore", [1, 3, 3, 1, 3, 1]),
    ]:
        buf = io.BytesIO()
        frames[0].save(
            buf,
            "GIF",
            save_all=True,
            append_images=frames[1:],
            duration=[100, 50, 0, 200, 10, 70],
            loop=0,
            disposal=disposal,
        )
        (HERE / f"{name}.gif").write_bytes(buf.getvalue())
        write_answer(name, *pillow_frames(buf.getvalue()))


def hand_written() -> None:
    rng = random.Random(1305)
    grey4 = [(0, 0, 0), (85, 85, 85), (170, 170, 170), (255, 255, 255)]
    colours256 = [(rng.randrange(256), rng.randrange(256), rng.randrange(256)) for _ in range(256)]
    noise = [rng.randrange(256) for _ in range(200 * 150)]

    cases: list[tuple[str, Gif, bool]] = [
        # The dictionary fills and the encoder carries on without a clear.
        ("gif_deferred_clear", Gif(200, 150, [Frame(200, 150, noise, clear_when_full=False)], palette=colours256), True),
        # A clear every 97 codes, mid-string and mid-row.
        ("gif_clears", Gif(200, 150, [Frame(200, 150, noise, clear_every=97)], palette=colours256), True),
        # Interlaced, by hand, at a height where every pass is uneven.
        ("gif_interlaced_odd", Gif(13, 11, [Frame(13, 11, [rng.randrange(4) for _ in range(143)], interlace=True)], palette=grey4), True),
        # 87a: no extensions at all.
        ("gif_87a", Gif(5, 3, [Frame(5, 3, [i % 4 for i in range(15)])], palette=grey4, version=b"87a"), True),
        # An image larger than the logical screen: the canvas grows to hold it.
        ("gif_oversize", Gif(3, 2, [Frame(5, 4, [i % 4 for i in range(20)])], palette=grey4), True),
        # No colour table anywhere: indices read as greys.
        ("gif_no_palette", Gif(4, 2, [Frame(4, 2, [0, 1, 2, 3, 200, 255, 17, 128], min_code_size=8)]), True),
        # Indices past a four-entry table: opaque black.
        ("gif_past_palette", Gif(4, 1, [Frame(4, 1, [1, 3, 5, 7], min_code_size=3)], palette=grey4), True),
        # Frames only partly on the canvas are clipped to it.
        ("gif_offscreen", Gif(6, 4, [
            Frame(6, 4, [1] * 24, disposal=1),
            Frame(4, 3, [2] * 12, left=4, top=2, disposal=1, delay=7),
            Frame(3, 3, [3] * 9, left=9, top=9, delay=3),
        ], palette=grey4), False),
        # A first image smaller than the screen: the rest of the canvas stays
        # transparent, where Pillow paints palette entry 0.
        ("gif_partial_first", Gif(5, 4, [Frame(2, 2, [3, 3, 3, 3], left=1, top=1)], palette=grey4), False),
        # Disposal 2 with no transparent index: browsers clear to transparent,
        # Pillow paints the background colour.
        ("gif_clear_opaque", Gif(4, 2, [
            Frame(4, 2, [1] * 8, disposal=2, delay=4),
            Frame(2, 1, [3, 3], left=1, delay=4),
        ], palette=grey4, background=2), False),
        # Disposal 3 on the first frame puts back the empty canvas; later
        # ones put back what was there.
        ("gif_restore_first", Gif(4, 2, [
            Frame(4, 2, [1] * 8, disposal=3, delay=5),
            Frame(2, 2, [2] * 4, left=2, disposal=1, delay=5),
            Frame(1, 2, [3, 3], left=0, disposal=3, delay=5),
            Frame(1, 1, [0], left=3, top=1, delay=5),
        ], palette=grey4), False),
        # Transparency over a kept frame, local palettes, a zero delay, and a
        # loop count other than forever.
        ("gif_local_palettes", Gif(3, 2, [
            Frame(3, 2, [0, 1, 2, 3, 2, 1], palette=[(255, 0, 0), (0, 255, 0), (0, 0, 255), (255, 255, 0)], disposal=1, delay=0),
            Frame(3, 2, [7, 5, 7, 4, 7, 6], palette=[(9, 9, 9)] * 4 + [(10, 20, 30), (40, 50, 60), (70, 80, 90), (1, 2, 3)], transparent=7, delay=1),
        ], loop=3), True),
    ]
    for name, gif, pillow_agrees in cases:
        data = gif.encode()
        (HERE / f"{name}.gif").write_bytes(data)
        answer = composite(gif)
        # The reference rules and Pillow agree wherever Pillow follows the
        # browsers; where it does not, the fixture says so, and the check
        # below makes sure it really does differ -- a fixture meant to show a
        # disagreement that shows none is testing nothing.
        agrees = same_as_pillow(data, answer)
        assert agrees == pillow_agrees, f"{name}: Pillow {'dis' if pillow_agrees else ''}agrees"
        write_answer(name, *answer)

    # A file cut off inside its only image's data: what arrived is drawn.
    whole = Gif(40, 30, [Frame(40, 30, [rng.randrange(4) for _ in range(1200)])], palette=grey4)
    data = whole.encode()
    cut = data[: len(data) * 2 // 3]
    (HERE / "gif_truncated.gif").write_bytes(cut)
    (HERE / "gif_truncated_whole.gif").write_bytes(data)
    write_answer("gif_truncated_whole", *composite(whole))


def main() -> None:
    pillow_written()
    hand_written()


if __name__ == "__main__":
    main()

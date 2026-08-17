r"""An independent `gvar` reader, used as the oracle for the Rust one.

Prints the expectation tables that `gui/font/tests/host_fonts.rs` compares
`Face::outline_at` against, for the variable fonts installed on this host:

    python gvar_oracle.py                 # both tables
    python gvar_oracle.py --fonts DIR     # somewhere other than C:\Windows\Fonts

Why a second implementation exists at all
-----------------------------------------

Everything below the `glyf` reader is written from the OpenType specification,
*not* transcribed from `gui/font/src/gvar.rs`. That is the whole point: a delta
the two agree on has been derived twice, independently, so a misreading of the
format has to be made twice, the same way, to survive. Checking the Rust
against numbers the Rust itself produced would only freeze today's behaviour --
including today's bugs.

The parts that are *not* independent, and how that is handled
-------------------------------------------------------------

The `glyf` reader and the point-to-path conversion here necessarily follow the
same rules the Rust does, and a slip in either would look exactly like a
variation bug. So this tool emits two tables. The first is the **default
instance**, where every tuple's scalar is provably zero and `gvar` therefore
contributes nothing: if the Rust and this tool agree there, the shared parts are
transcribed correctly, and any disagreement in the *second* table is variation
and nothing else.

What a row contains, and why not the whole outline
--------------------------------------------------

Per (face, glyph, instance): the command count, the bounding box, and the sums
of all x and all y coordinates the path touches. The count catches a structural
change, the box catches a shift or a scale, and the sums catch a single wrong
point anywhere -- including an interior one that never reaches the box. Storing
every coordinate would be some thousands of numbers per face and no more
conclusive; storing a hash would be equally compact but would say only "wrong",
never "wrong how".

The coordinates are compared with a tolerance rather than for equality: this
tool accumulates in float64 and the Rust in f32, so the last bits legitimately
differ, and a test that demanded they not would be reporting on IEEE 754 rather
than on `gvar`.
"""

import argparse
import glob
import os
import struct
import sys

from variable_survey import (
    apply_segment_map,
    f2dot14,
    i16,
    normalize_user,
    read_avar_segments,
    read_fvar,
    read_fvar_instances,
    tables,
    u16,
    u32,
)

MAX_DEPTH = 8
PHANTOM_COUNT = 4


# ---------------------------------------------------------------------------
# `glyf`, `loca`, `hmtx` -- shared with the Rust, validated at the default
# instance rather than claimed to be independent.
# ---------------------------------------------------------------------------


def read_num_glyphs(data, tabs):
    if "maxp" not in tabs:
        return None
    off = tabs["maxp"][0]
    return u16(data, off + 4) if off + 6 <= len(data) else None


def read_loca_long(data, tabs):
    if "head" not in tabs:
        return None
    off = tabs["head"][0]
    if off + 54 > len(data):
        return None
    fmt = i16(data, off + 50)
    return fmt == 1 if fmt in (0, 1) else None


def glyf_span(data, tabs, gid, long_loca, num_glyphs):
    """Byte range of `gid` in `glyf`, or None when the glyph draws nothing."""
    if gid >= num_glyphs or "loca" not in tabs or "glyf" not in tabs:
        return None
    lo = tabs["loca"][0]
    go, glen = tabs["glyf"]
    if long_loca:
        a = lo + gid * 4
        if a + 8 > len(data):
            return None
        start, end = u32(data, a), u32(data, a + 4)
    else:
        a = lo + gid * 2
        if a + 4 > len(data):
            return None
        start, end = u16(data, a) * 2, u16(data, a + 2) * 2
    if end <= start or go + end > len(data) or end - start > glen:
        return None
    return go + start, end - start


def read_hmtx_lsb(data, tabs, gid):
    if "hhea" not in tabs or "hmtx" not in tabs:
        return 0
    ho = tabs["hhea"][0]
    if ho + 36 > len(data):
        return 0
    n = u16(data, ho + 34)
    if n == 0:
        return 0
    mo = tabs["hmtx"][0]
    at = mo + gid * 4 + 2 if gid < n else mo + n * 4 + (gid - n) * 2
    return i16(data, at) if at + 2 <= len(data) else 0


def read_simple_glyph(body, n_contours):
    """Return (points, ends); a point is a mutable [x, y, on_curve]."""
    if n_contours == 0 or len(body) < n_contours * 2 + 2:
        return None
    ends = [u16(body, i * 2) for i in range(n_contours)]
    n_points = ends[-1] + 1
    pos = n_contours * 2 + 2 + u16(body, n_contours * 2)
    if pos > len(body):
        return None

    flags = []
    while len(flags) < n_points:
        if pos >= len(body):
            return None
        f = body[pos]
        pos += 1
        flags.append(f)
        if f & 0x08:  # REPEAT_FLAG
            if pos >= len(body):
                return None
            repeat = body[pos]
            pos += 1
            for _ in range(repeat):
                if len(flags) >= n_points:
                    break
                flags.append(f)
    if len(flags) != n_points:
        return None

    def axis(short_bit, same_bit):
        """One coordinate, delta-encoded, 0/1/2 bytes per point."""
        nonlocal pos
        vals = []
        v = 0
        for f in flags:
            if f & short_bit:
                if pos >= len(body):
                    return None
                v += body[pos] if (f & same_bit) else -body[pos]
                pos += 1
            elif not (f & same_bit):
                if pos + 2 > len(body):
                    return None
                v += i16(body, pos)
                pos += 2
            vals.append(v)
        return vals

    xs = axis(0x02, 0x10)  # X_SHORT_VECTOR, X_SAME_OR_POSITIVE_X_SHORT_VECTOR
    ys = axis(0x04, 0x20) if xs is not None else None
    if ys is None:
        return None
    points = [[float(xs[i]), float(ys[i]), bool(flags[i] & 0x01)] for i in range(n_points)]
    return points, [e + 1 for e in ends]


def f2dot14_at(b, o):
    return i16(b, o) / 16384.0


def read_components(body):
    """Return [[gid, a, b, c, d, dx, dy, offset_placed], ...]."""
    out = []
    pos = 0
    while True:
        if pos + 4 > len(body):
            return None
        flags = u16(body, pos)
        gid = u16(body, pos + 2)
        pos += 4
        if flags & 0x0001:  # ARG_1_AND_2_ARE_WORDS
            if pos + 4 > len(body):
                return None
            a1, a2 = i16(body, pos), i16(body, pos + 2)
            pos += 4
        else:
            if pos + 2 > len(body):
                return None
            a1 = struct.unpack_from(">b", body, pos)[0]
            a2 = struct.unpack_from(">b", body, pos + 1)[0]
            pos += 2
        xy = bool(flags & 0x0002)  # ARGS_ARE_XY_VALUES
        dx, dy = (float(a1), float(a2)) if xy else (0.0, 0.0)
        if flags & 0x0008:  # WE_HAVE_A_SCALE
            if pos + 2 > len(body):
                return None
            sa = sd = f2dot14_at(body, pos)
            sb = sc = 0.0
            pos += 2
        elif flags & 0x0040:  # WE_HAVE_AN_X_AND_Y_SCALE
            if pos + 4 > len(body):
                return None
            sa, sd = f2dot14_at(body, pos), f2dot14_at(body, pos + 2)
            sb = sc = 0.0
            pos += 4
        elif flags & 0x0080:  # WE_HAVE_A_TWO_BY_TWO
            if pos + 8 > len(body):
                return None
            sa, sb = f2dot14_at(body, pos), f2dot14_at(body, pos + 2)
            sc, sd = f2dot14_at(body, pos + 4), f2dot14_at(body, pos + 6)
            pos += 8
        else:
            sa = sd = 1.0
            sb = sc = 0.0
        out.append([gid, sa, sb, sc, sd, dx, dy, xy])
        if not (flags & 0x0020):  # MORE_COMPONENTS
            break
    return out


# ---------------------------------------------------------------------------
# `gvar` -- written from the specification.
# ---------------------------------------------------------------------------


def read_gvar_header(data, tabs, axis_count, num_glyphs):
    if "gvar" not in tabs:
        return None
    base, length = tabs["gvar"]
    if length < 20 or base + 20 > len(data):
        return None
    if u16(data, base) != 1 or u16(data, base + 4) != axis_count:
        return None
    glyph_count = u16(data, base + 12)
    if glyph_count > num_glyphs:
        return None
    return {
        "axis_count": axis_count,
        "shared": base + u32(data, base + 8),
        "shared_count": u16(data, base + 6),
        "offsets": base + 20,
        "array": base + u32(data, base + 16),
        "glyph_count": glyph_count,
        "long": bool(u16(data, base + 14) & 1),
    }


def gvar_glyph_span(data, hdr, gid):
    if gid >= hdr["glyph_count"]:
        return None

    def read(k):
        if hdr["long"]:
            at = hdr["offsets"] + k * 4
            return u32(data, at) if at + 4 <= len(data) else None
        at = hdr["offsets"] + k * 2
        return u16(data, at) * 2 if at + 2 <= len(data) else None

    a, b = read(gid), read(gid + 1)
    if a is None or b is None or b <= a:
        return None
    off = hdr["array"] + a
    return (off, b - a) if off + (b - a) <= len(data) else None


def read_tuple(b, off, count):
    if off + count * 2 > len(b):
        return None
    return [i16(b, off + i * 2) for i in range(count)]


def gvar_scalar(peak, region, coords):
    """How much of a tuple applies at `coords`: 1 at its peak, 0 outside."""
    scale = 1.0
    for i, pk in enumerate(peak):
        if pk == 0:
            continue
        v = coords[i] if i < len(coords) else 0
        if v == pk:
            continue
        if region is None:
            if v == 0 or v < min(pk, 0) or v > max(pk, 0):
                return 0.0
            scale *= v / pk
        else:
            s, e = region[0][i], region[1][i]
            if s > pk or pk > e or (s < 0 < e and pk != 0):
                continue
            if v < s or v > e:
                return 0.0
            if v < pk:
                if pk != s:
                    scale *= (v - s) / (pk - s)
            elif pk != e:
                scale *= (e - v) / (e - pk)
    return scale


def read_packed_points(g, pos, total):
    """Return (selection, new_pos); selection is "all" or a list of numbers."""
    if pos >= len(g):
        return None, pos
    n = g[pos]
    pos += 1
    if n & 0x80:  # POINTS_ARE_WORDS -- a two-byte count
        if pos >= len(g):
            return None, pos
        n = ((n & 0x7F) << 8) | g[pos]
        pos += 1
    if n == 0:
        return "all", pos
    out = []
    prev = 0
    while len(out) < n:
        if pos >= len(g):
            return None, pos
        ctrl = g[pos]
        pos += 1
        words = bool(ctrl & 0x80)
        for _ in range((ctrl & 0x7F) + 1):
            if len(out) >= n:
                break
            if words:
                if pos + 2 > len(g):
                    return None, pos
                prev += u16(g, pos)
                pos += 2
            else:
                if pos >= len(g):
                    return None, pos
                prev += g[pos]
                pos += 1
            out.append(prev)
    if any(p >= total for p in out):
        return None, pos
    return out, pos


def read_packed_deltas(g, pos, count):
    out = []
    while len(out) < count:
        if pos >= len(g):
            return None, pos
        ctrl = g[pos]
        pos += 1
        run = (ctrl & 0x3F) + 1
        if len(out) + run > count:
            return None, pos
        if ctrl & 0x80:  # DELTAS_ARE_ZERO
            out += [0] * run
        elif ctrl & 0x40:  # DELTAS_ARE_WORDS
            if pos + run * 2 > len(g):
                return None, pos
            for _ in range(run):
                out.append(i16(g, pos))
                pos += 2
        else:
            if pos + run > len(g):
                return None, pos
            for _ in range(run):
                out.append(struct.unpack_from(">b", g, pos)[0])
                pos += 1
    return out, pos


def infer(target, prev, nxt, d_prev, d_next):
    """One coordinate of an unnamed point, from its two named neighbours."""
    if prev == nxt:
        return d_prev if d_prev == d_next else 0.0
    if target <= min(prev, nxt):
        return d_prev if prev < nxt else d_next
    if target >= max(prev, nxt):
        return d_prev if prev > nxt else d_next
    ratio = (target - prev) / (nxt - prev)
    return d_prev + ratio * (d_next - d_prev)


def interpolate_unnamed(points, ends, named, delta):
    """IUP: unnamed points follow the named ones on either side of them."""
    start = 0
    for end in ends:
        span = list(range(start, end))
        start = end
        n = len(span)
        if n == 0:
            continue
        marked = [k for k in span if named[k]]
        # Nothing to interpolate from, or nothing left to interpolate.
        if not marked or len(marked) == n:
            continue
        for pos, k in enumerate(span):
            if named[k]:
                continue
            before = next(span[(pos - s) % n] for s in range(1, n + 1)
                          if named[span[(pos - s) % n]])
            after = next(span[(pos + s) % n] for s in range(1, n + 1)
                         if named[span[(pos + s) % n]])
            delta[k] = (
                infer(points[k][0], points[before][0], points[after][0],
                      delta[before][0], delta[after][0]),
                infer(points[k][1], points[before][1], points[after][1],
                      delta[before][1], delta[after][1]),
            )
    return delta


def gvar_deltas(data, hdr, gid, coords, points, ends):
    """Accumulated (dx, dy) per point of `gid`, phantom points included."""
    span = gvar_glyph_span(data, hdr, gid)
    if span is None:
        return None
    off, length = span
    g = data[off:off + length]
    if len(g) < 4:
        return None
    total = len(points) + PHANTOM_COUNT
    count_word = u16(g, 0)
    n_tuples = count_word & 0x0FFF
    if n_tuples == 0:
        return None
    serial = u16(g, 2)

    shared = None
    if count_word & 0x8000:  # SHARED_POINT_NUMBERS
        shared, serial = read_packed_points(g, serial, total)
        if shared is None:
            return None

    out = [(0.0, 0.0)] * total
    header = 4
    axes = hdr["axis_count"]
    for _ in range(n_tuples):
        if header + 4 > len(g):
            return None
        size = u16(g, header)
        index = u16(g, header + 2)
        h = header + 4
        if index & 0x8000:  # EMBEDDED_PEAK_TUPLE
            peak = read_tuple(g, h, axes)
            h += axes * 2
        else:
            k = index & 0x0FFF
            if k >= hdr["shared_count"]:
                return None
            peak = read_tuple(data, hdr["shared"] + k * axes * 2, axes)
        if peak is None:
            return None
        region = None
        if index & 0x4000:  # INTERMEDIATE_REGION
            lo = read_tuple(g, h, axes)
            hi = read_tuple(g, h + axes * 2, axes)
            h += axes * 4
            if lo is None or hi is None:
                return None
            region = (lo, hi)
        header = h

        scale = gvar_scalar(peak, region, coords)
        # A tuple that scores zero still occupies its bytes: the next tuple's
        # data begins where this one's ends, not where its own header says.
        if scale != 0.0:
            p = serial
            if index & 0x2000:  # PRIVATE_POINT_NUMBERS
                selection, p = read_packed_points(g, p, total)
                if selection is None:
                    return None
            else:
                selection = shared if shared is not None else "all"
            if selection == "all":
                xs, p = read_packed_deltas(g, p, total)
                if xs is None:
                    return None
                ys, p = read_packed_deltas(g, p, total)
                if ys is None:
                    return None
                out = [(out[i][0] + scale * xs[i], out[i][1] + scale * ys[i])
                       for i in range(total)]
            else:
                xs, p = read_packed_deltas(g, p, len(selection))
                if xs is None:
                    return None
                ys, p = read_packed_deltas(g, p, len(selection))
                if ys is None:
                    return None
                delta = [(0.0, 0.0)] * total
                named = [False] * total
                for k, num in enumerate(selection):
                    delta[num] = (float(xs[k]), float(ys[k]))
                    named[num] = True
                interpolate_unnamed(points, ends, named, delta)
                out = [(out[i][0] + scale * delta[i][0],
                        out[i][1] + scale * delta[i][1]) for i in range(total)]
        serial += size
    return out


# ---------------------------------------------------------------------------
# Points to a path, and a path to a comparable record.
# ---------------------------------------------------------------------------


def emit_path(points, ends):
    """(command count, every coordinate the path touches), in crate order."""
    cmds = 0
    coords = []

    def push(*pts):
        nonlocal cmds
        cmds += 1
        coords.extend(pts)

    start = 0
    for end in ends:
        contour = points[start:end]
        start = end
        n = len(contour)
        if n == 0:
            continue
        on_curve = [i for i, p in enumerate(contour) if p[2]]
        if on_curve:
            i = on_curve[0]
            anchor = (contour[i][0], contour[i][1])
            walk = [contour[(i + 1 + k) % n] for k in range(n - 1)]
        else:
            # Every point is a control point: the path starts at the implied
            # midpoint of the last and the first.
            first, last = contour[0], contour[-1]
            anchor = ((first[0] + last[0]) / 2.0, (first[1] + last[1]) / 2.0)
            walk = list(contour)
        push(anchor)
        ctrl = None
        for pt in walk:
            p = (pt[0], pt[1])
            if pt[2]:
                if ctrl is None:
                    push(p)
                else:
                    push(ctrl, p)
                    ctrl = None
            else:
                if ctrl is not None:
                    push(ctrl, ((ctrl[0] + p[0]) / 2.0, (ctrl[1] + p[1]) / 2.0))
                ctrl = p
        if ctrl is None:
            push(anchor)
        else:
            push(ctrl, anchor)
        cmds += 1  # Close
    return cmds, coords


def varied_points(data, tabs, hdr, gid, coords, long_loca, num_glyphs, depth=0):
    """(points, ends, phantom_left_dx) for `gid`, composites resolved."""
    if depth > MAX_DEPTH:
        return None
    span = glyf_span(data, tabs, gid, long_loca, num_glyphs)
    if span is None:
        return [], [], 0.0
    off, length = span
    g = data[off:off + length]
    if len(g) < 10:
        return None
    n_contours = i16(g, 0)
    body = g[10:]

    if n_contours >= 0:
        parsed = read_simple_glyph(body, n_contours)
        if parsed is None:
            return [], [], 0.0
        points, ends = parsed
        phantom = 0.0
        if hdr is not None and coords is not None:
            deltas = gvar_deltas(data, hdr, gid, coords, points, ends)
            if deltas is not None:
                for i, pt in enumerate(points):
                    pt[0] += deltas[i][0]
                    pt[1] += deltas[i][1]
                phantom = deltas[len(points)][0]
        return points, ends, phantom

    comps = read_components(body)
    if comps is None:
        return None
    phantom = 0.0
    if hdr is not None and coords is not None:
        # A composite's variation points are its components' offsets: one each,
        # no contours, so no interpolation.
        placements = [[c[5], c[6], True] for c in comps]
        deltas = gvar_deltas(data, hdr, gid, coords, placements, [])
        if deltas is not None:
            for i, c in enumerate(comps):
                if c[7]:
                    c[5] += deltas[i][0]
                    c[6] += deltas[i][1]
            phantom = deltas[len(comps)][0]
    points, ends = [], []
    for gid_c, a, b, c, d, e, f, _xy in comps:
        sub = varied_points(data, tabs, hdr, gid_c, coords, long_loca, num_glyphs,
                            depth + 1)
        if sub is None:
            return None
        child_points, child_ends, _ = sub
        base = len(points)
        for pt in child_points:
            points.append([a * pt[0] + c * pt[1] + e, b * pt[0] + d * pt[1] + f, pt[2]])
        ends += [base + x for x in child_ends]
    return points, ends, phantom


def outline_record(data, tabs, hdr, gid, coords, long_loca, num_glyphs):
    """(cmds, x_min, y_min, x_max, y_max, sum_x, sum_y) for one glyph."""
    result = varied_points(data, tabs, hdr, gid, coords, long_loca, num_glyphs)
    if result is None:
        return None
    points, ends, phantom = result
    if not points:
        return None
    span = glyf_span(data, tabs, gid, long_loca, num_glyphs)
    if span is None:
        return None
    x_min = i16(data, span[0] + 2)
    # The outline is placed so the left side bearing point lands on the origin;
    # that point moves with the glyph, hence the phantom correction.
    shift = read_hmtx_lsb(data, tabs, gid) - x_min - phantom
    cmds, coordinates = emit_path(points, ends)
    if not coordinates:
        return None
    xs = [c[0] + shift for c in coordinates]
    ys = [c[1] for c in coordinates]
    return cmds, min(xs), min(ys), max(xs), max(ys), sum(xs), sum(ys)


def pick_glyphs(data, tabs, hdr, long_loca, num_glyphs):
    """Two varying simple glyphs and one varying composite, lowest gids."""
    simple, composite = [], []
    for gid in range(min(num_glyphs, hdr["glyph_count"])):
        if len(simple) >= 2 and composite:
            break
        if gvar_glyph_span(data, hdr, gid) is None:
            continue
        span = glyf_span(data, tabs, gid, long_loca, num_glyphs)
        if span is None:
            continue
        g = data[span[0]:span[0] + span[1]]
        if len(g) < 10:
            continue
        n = i16(g, 0)
        if n >= 0:
            if len(simple) < 2:
                parsed = read_simple_glyph(g[10:], n)
                # A glyph with a handful of points exercises nothing; ask for
                # enough that a partial tuple has something to interpolate.
                if parsed is not None and len(parsed[0]) >= 8:
                    simple.append(gid)
        elif not composite:
            composite.append(gid)
    return simple + composite


def fmt(rec):
    cmds, x0, y0, x1, y1, sx, sy = rec
    return (f"{cmds}, {x0:.1f}, {y0:.1f}, {x1:.1f}, {y1:.1f}, "
            f"{sx:.1f}, {sy:.1f}")


def report(paths):
    default_rows, varied_rows = [], []
    for path in paths:
        try:
            with open(path, "rb") as fh:
                data = fh.read()
        except OSError:
            continue
        tabs = tables(data)
        if tabs is None or "fvar" not in tabs or "gvar" not in tabs:
            continue
        parsed = read_fvar(data, *tabs["fvar"])
        if parsed is None:
            continue
        axes = parsed[0]
        num_glyphs = read_num_glyphs(data, tabs)
        long_loca = read_loca_long(data, tabs)
        if num_glyphs is None or long_loca is None:
            continue
        hdr = read_gvar_header(data, tabs, len(axes), num_glyphs)
        if hdr is None:
            continue
        segments = (read_avar_segments(data, tabs["avar"][0], len(axes))
                    if "avar" in tabs else None)
        instances = read_fvar_instances(data, *tabs["fvar"], len(axes))
        name = os.path.basename(path)
        gids = pick_glyphs(data, tabs, hdr, long_loca, num_glyphs)

        for gid in gids:
            rec = outline_record(data, tabs, None, gid, None, long_loca, num_glyphs)
            if rec is not None:
                default_rows.append(f'    ("{name}", {gid}, {fmt(rec)}),')

        # First, middle and last named instance: the ends of each axis and one
        # interior point, which is where `avar` and a tapering tuple both bite.
        picks = sorted({0, len(instances) // 2, len(instances) - 1}) if instances else []
        for ii in picks:
            user = instances[ii][1]
            coords = []
            for i, axis in enumerate(axes):
                c = f2dot14(normalize_user(axis, user[i]))
                if segments is not None:
                    c = apply_segment_map(segments[i], c)
                coords.append(c)
            for gid in gids:
                rec = outline_record(data, tabs, hdr, gid, coords, long_loca, num_glyphs)
                if rec is not None:
                    shown = ", ".join(f"{v:g}" for v in user)
                    varied_rows.append(
                        f'    ("{name}", {gid}, {ii}, {fmt(rec)}),  // {shown}')

    print("// ---- default instance: validates this tool's own glyf reader ----")
    for row in default_rows:
        print(row)
    print()
    print("// ---- named instances: validates `gvar` ----")
    for row in varied_rows:
        print(row)


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--fonts", default=r"C:\Windows\Fonts")
    args = ap.parse_args()
    paths = []
    for ext in ("ttf", "otf", "ttc", "otc"):
        paths += glob.glob(os.path.join(args.fonts, f"*.{ext}"))
        paths += glob.glob(os.path.join(args.fonts, f"*.{ext.upper()}"))
    paths = sorted(set(paths))
    if not paths:
        print(f"no fonts found under {args.fonts}", file=sys.stderr)
        return 1
    report(paths)
    return 0


if __name__ == "__main__":
    sys.exit(main())

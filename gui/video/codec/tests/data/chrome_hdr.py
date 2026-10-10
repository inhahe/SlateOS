"""Chrome's handling of an HDR picture on an ordinary (SDR, sRGB) screen,
transcribed from Skia in double precision: the reference `gui/video/yuv`'s
`hdr` module is held to (design-decisions 1378), and from which
`generate_hdr_fixtures.py` makes videocodec's HDR answers.

Checked against Chrome 154 itself: lossless 10-bit 4:4:4 AVIFs of 24 PQ and
16 HLG patches, shown in headless Chrome and read back from its screenshots,
agree with `pixel` exactly (the same patches are `yuv::hdr`'s unit tests);
and VP9 WebMs of them, played in a <video>, within one (Chrome's video path
converts Y'CbCr on the GPU).

  1. Y'CbCr to R'G'B' -- the caller's: `generate_hdr_fixtures.py` does it as
     libavif's floating point does, over libyuv's chroma upsampling.
  2. Into the tone map's working space, linear BT.2020 with 1.0 at the HDR
     reference white (203 cd/m2): SkColorSpaceXformSteps -- PQ by skcms's
     PQish curve (light over 10 000 cd/m2) times 10000/203; HLG by its
     HLGish curve (BT.2100's inverse OETF), BT.2100's OOTF (gamma 1.2,
     BT.2100's luminance weights) and 1000/203.
  3. skhdr's default tone map (SkHdrAgtm.cpp): RWTMO, the content's headroom
     log2(peak/203) from MaxCLL, else the mastering display's peak, else
     1000; for a screen of no headroom, alternate image 0's gain curve,
     on max(R, G, B) -- its control points computed in single precision as
     Skia computes them and rounded to half precision, as Skia's shader reads
     them from an F16 texture.
  4. To sRGB: Skia's gamut matrices (to XYZ D50) and the sRGB curve's
     inverse; clamped; 8 bits.

Only BT.2020's primaries are here; `yuv::hdr` carries the others as Skia
does, and its unit tests check them.
"""
import math
import struct

SDR_WHITE = 203.0
KAPPA = 0.65


def f32(x):
    return struct.unpack("<f", struct.pack("<f", x))[0]


def f16(x):
    return struct.unpack("<e", struct.pack("<e", x))[0]


# --- 2. the curves, as skcms evaluates them ------------------------------

PQISH = (-107 / 128, 1.0, 32 / 2523, 2413 / 128, -2392 / 128, 8192 / 1305)  # A B C D E F


def pqish(x):
    """skcms PQish: light over 10 000 cd/m2 (negative numerators taken as 0)."""
    a, b, c, d, e, f = PQISH
    xc = x ** c if x > 0 else 0.0
    num = max(a + b * xc, 0.0)
    return (num / (d + e * xc)) ** f


HLG_A, HLG_B, HLG_C = 0.17883277, 0.28466892, 0.55991073


def hlgish(x):
    """skcms HLGish scaled by 1/12: BT.2100's inverse OETF."""
    return (x * x / 3.0) if x <= 0.5 else (math.exp((x - HLG_C) / HLG_A) + HLG_B) / 12.0


def to_working(rgb, transfer):
    """R'G'B' (BT.2020) to linear BT.2020, 1.0 at 203 cd/m2."""
    if transfer == 16:
        return [pqish(c) * (10000.0 / SDR_WHITE) for c in rgb]
    if transfer == 18:
        s = [hlgish(c) for c in rgb]
        y = 0.2627 * s[0] + 0.6780 * s[1] + 0.0593 * s[2]
        g = y ** 0.2 if y > 0 else 0.0
        return [c * g * (1000.0 / SDR_WHITE) for c in s]
    raise ValueError(transfer)


# --- 3. skhdr's default tone map -------------------------------------------

def peak_luminance(max_cll=None, mastering_peak=None):
    """get_peak_luminance: MaxCLL, else the mastering display's, else 1000."""
    if max_cll and max_cll > 0:
        return float(max_cll)
    if mastering_peak and mastering_peak > 0:
        return float(mastering_peak)
    return 1000.0


def baseline_headroom(peak):
    """In single precision, as PopulateToneMapAgtmParams computes it."""
    return f32(min(math.log2(f32(f32(peak) / f32(SDR_WHITE))), 6.0)) if peak > SDR_WHITE else 0.0


def rwtmo_alt0(baseline_headroom, half=True):
    """Alternate image 0's gain curve (control points x, y, m) under RWTMO,
    as PopulateUsingRwtmo makes it -- in single precision, as Skia does; and,
    with `half`, rounded to half precision, as the shader's texture holds it."""
    hb = baseline_headroom
    if hb == 0.0:
        return None
    ratio = min(hb / math.log2(1000 / 203.0), 1.0)
    y_white = f32(1.0 - 0.5 * ratio)
    x_knee, y_knee = 1.0, y_white
    x_max, y_max = f32(2.0 ** hb), 1.0  # exp2(0) for alternate image 0
    x_mid = f32((1 - KAPPA) * x_knee + KAPPA * (x_knee * y_max / y_knee))
    y_mid = f32((1 - KAPPA) * y_knee + KAPPA * y_max)
    xa, ya = f32(x_knee - 2 * x_mid + x_max), f32(y_knee - 2 * y_mid + y_max)
    xb, yb = f32(2 * x_mid - 2 * x_knee), f32(2 * y_mid - 2 * y_knee)
    xc, yc = x_knee, y_knee
    points = []
    for c in range(8):
        t = f32(c / 7.0)
        x = f32(xc + t * (xb + t * xa))
        y = f32(yc + t * (yb + t * ya))
        m = f32((2 * ya * t + yb) / (2 * xa * t + xb))
        point = (x, f32(math.log2(y / x)), f32((x * m - y) / (math.log(2.0) * x * y)))
        points.append(tuple(f16(v) for v in point) if half else point)
    return points


def gain(points, x):
    """EvaluateGainCurve: log2 of the gain at x."""
    if x <= points[0][0]:
        return points[0][1]
    if x >= points[-1][0]:
        return points[-1][1] + math.log2(points[-1][0] / x)
    i, j = 0, len(points) - 1
    while j - i > 1:
        m = (i + j) // 2
        if x < points[m][0]:
            j = m
        else:
            i = m
    (xi, yi, mi), (xj, yj, mj) = points[i], points[j]
    h = xj - xi
    if h == 0:
        return yi
    mhi, mhj = mi * h, mj * h
    c3 = 2 * yi + mhi - 2 * yj + mhj
    c2 = -3 * yi + 3 * yj - 2 * mhi - mhj
    t = (x - xi) / h
    return ((c3 * t + c2) * t + mhi) * t + yi


def tone_map(lin, points):
    if points is None:
        return lin
    g = gain(points, max(lin))
    return [c * 2.0 ** g for c in lin]


# --- 4. to sRGB ---------------------------------------------------------------

def fixed(v):
    return v / 65536.0


SRGB_TO_XYZD50 = [[fixed(0x6FA2), fixed(0x6299), fixed(0x24A0)],
                  [fixed(0x38F5), fixed(0xB785), fixed(0x0F84)],
                  [fixed(0x0390), fixed(0x18DA), fixed(0xB6CF)]]
REC2020_TO_XYZD50 = [[0.673459, 0.165661, 0.125100],
                     [0.279033, 0.675338, 0.0456288],
                     [-0.00193139, 0.0299794, 0.797162]]


def inv3(m):
    (a, b, c), (d, e, f), (g, h, i) = m
    det = a * (e * i - f * h) - b * (d * i - f * g) + c * (d * h - e * g)
    return [[(e * i - f * h) / det, (c * h - b * i) / det, (b * f - c * e) / det],
            [(f * g - d * i) / det, (a * i - c * g) / det, (c * d - a * f) / det],
            [(d * h - e * g) / det, (b * g - a * h) / det, (a * e - b * d) / det]]


def mul3(a, b):
    return [[sum(a[r][k] * b[k][c] for k in range(3)) for c in range(3)] for r in range(3)]


REC2020_TO_SRGB = mul3(inv3(SRGB_TO_XYZD50), REC2020_TO_XYZD50)


def srgb_encode(l):
    l = min(max(l, 0.0), 1.0)
    return 12.92 * l if l < 0.0031308 else 1.055 * l ** (1 / 2.4) - 0.055


def to_srgb8(lin):
    out = [sum(REC2020_TO_SRGB[r][k] * lin[k] for k in range(3)) for r in range(3)]
    return [int(srgb_encode(c) * 255 + 0.5) for c in out]


def pixel(rgb, transfer, points):
    """R'G'B' (BT.2020, each 0 to 1) to 8-bit sRGB, as Chrome shows it."""
    return to_srgb8(tone_map(to_working(rgb, transfer), points))

"""Read constant tables out of C source and write them as Rust.

The video codecs ported from C -- gui/video/vp8 and gui/video/vp9, both from
libvpx -- need thousands of numbers that are the C library's and nobody
else's: default probabilities, scan orders, quantiser steps, filter kernels.
Typing them would be the one part of a port nobody could check, so each
codec has a `tools/gen_tables.py` that lists the tables it needs and hands the
list to `generate` here, which reads them out of a checkout of the C source:

    python tools/gen_tables.py path/to/libvpx > src/tables.rs

Each array is read by its C name, filled by C's own initializer rules (nested
braces, brace elision, missing trailing elements zero), and checked against
the shape the Rust side declares: a table that does not fit is an error that
names it, never a truncation or a guess. A struct whose only member is an
array (libvpx's `MV_CONTEXT`) adds a level of braces; its place in a shape is
written `STRUCT`, which reads that level and leaves it out of the Rust type.

The output goes through rustfmt, so a regenerated file diffs cleanly against
the committed one: run the generator and compare whenever a table is in doubt.
"""

import re
import struct
import subprocess
from fractions import Fraction

#: A shape entry for a struct that wraps one array: a level of braces in C,
#: no level in Rust.
STRUCT = "struct"


def initializer(source: str, name: str) -> str:
    """The text between the braces of `name`'s initializer."""
    m = re.search(r"\b" + re.escape(name) + r"\s*\[[^=;]*=\s*\{", source)
    if not m:
        raise SystemExit(f"{name}: not found")
    depth = 0
    start = m.end() - 1
    for i in range(start, len(source)):
        if source[i] == "{":
            depth += 1
        elif source[i] == "}":
            depth -= 1
            if depth == 0:
                return source[start:i + 1]
    raise SystemExit(f"{name}: unbalanced braces")


def strip_comments(text: str) -> str:
    text = re.sub(r"/\*.*?\*/", " ", text, flags=re.S)
    return re.sub(r"//[^\n]*", " ", text)


TOKEN = re.compile(r"\s*(\{|\}|,"
                   r"|-?\s*(?:\d+\.\d*|\.\d+)(?:[eE][-+]?\d+)?[fF]?"
                   r"|-?\s*\d+[eE][-+]?\d+[fF]?"
                   r"|-?\s*0[xX][0-9a-fA-F]+[uUlL]*|-?\s*\d+[uUlL]*|-?\s*[A-Za-z_]\w*)")


class CFloat:
    """A floating-point literal, kept as written: what it is worth depends on
    the type it initializes (see `c_float`)."""

    def __init__(self, text: str):
        self.text = text

    def __repr__(self):
        return f"CFloat({self.text})"


class CName:
    """An identifier in an initializer: another table, in a table of pointers."""

    def __init__(self, name: str):
        self.name = name

    def __repr__(self):
        return f"CName({self.name})"


def f32(x: float) -> float:
    """`x` rounded to the nearest float, ties to even, as C converts a double."""
    return struct.unpack("<f", struct.pack("<f", x))[0]


def c_float(lit: CFloat) -> float:
    """The value an element of a C `float` array takes from `lit`: a double
    literal rounded to the nearest double and then to a float; a float literal
    (suffix `f`) rounded to the nearest float straight from its decimal --
    which is not always the same, so it is found exactly."""
    text = lit.text.replace(" ", "")
    if text[-1] not in "fF":
        return f32(float(text))
    exact = Fraction(text[:-1])
    near = f32(float(exact))
    bits = struct.unpack("<I", struct.pack("<f", near))[0]
    # The float the double rounded to, and its neighbours: the nearest of
    # them to the exact value, ties to the even bit pattern.
    candidates = [struct.unpack("<f", struct.pack("<I", b))[0]
                  for b in (bits - 1, bits, bits + 1) if 0 <= b < 1 << 32]
    return min(candidates, key=lambda c: (abs(Fraction(c) - exact),
                                          struct.unpack("<I", struct.pack("<f", c))[0] & 1))


def q_format(lit, frac_bits: int) -> int:
    """FFmpeg's `Q30`/`Q31` of a C `float` array's element:
    `(int)((x) * 2^frac_bits + 0.5)`, the float promoted to double, the sum
    rounded as a double, and the conversion truncating toward zero."""
    if not isinstance(lit, CFloat):
        lit = CFloat(str(lit))
    return int(c_float(lit) * float(1 << frac_bits) + 0.5)


def parse(text: str, symbols: dict, keep_names: bool = False):
    """The initializer as nested lists of ints -- or `CFloat`s for float
    literals; names are looked up in `symbols`, or with `keep_names` kept as
    `CName`s."""
    pos = 0
    tokens = []
    while pos < len(text):
        m = TOKEN.match(text, pos)
        if not m:
            if text[pos:].strip() == "":
                break
            raise SystemExit(f"cannot read {text[pos:pos + 40]!r}")
        tokens.append(m.group(1).replace(" ", ""))
        pos = m.end()

    def value(tok: str):
        neg = tok.startswith("-")
        if re.fullmatch(r"-?(?:\d+\.\d*|\.\d+)(?:[eE][-+]?\d+)?[fF]?|-?\d+[eE][-+]?\d+[fF]?", tok):
            return CFloat(tok)
        body = tok.lstrip("-")
        if body[0].isdigit():
            # An integer's suffixes; an identifier keeps its letters.
            body = body.rstrip("uUlL")
        if body.lower().startswith("0x"):
            v = int(body, 16)
        elif body[0].isdigit():
            v = int(body)
        elif keep_names and not neg:
            return CName(body)
        elif body in symbols:
            v = symbols[body]
        else:
            raise SystemExit(f"unknown symbol {body}")
        return -v if neg else v

    def group(i: int):
        assert tokens[i] == "{"
        items = []
        i += 1
        while tokens[i] != "}":
            if tokens[i] == "{":
                sub, i = group(i)
                items.append(sub)
            elif tokens[i] == ",":
                i += 1
                continue
            else:
                items.append(value(tokens[i]))
                i += 1
        return items, i + 1

    tree, _ = group(0)
    return tree


def fill(items, shape, name):
    """C's initializer rules: nested braces, brace elision, zero padding."""

    def take(cursor, shape):
        # `cursor` is [list, index]: the items at one brace level.
        lst, _ = cursor
        if not shape:
            if cursor[1] >= len(lst):
                return 0
            item = lst[cursor[1]]
            cursor[1] += 1
            while isinstance(item, list):  # a scalar in braces
                item = item[0] if item else 0
            return item
        out = []
        for _ in range(shape[0]):
            if cursor[1] < len(lst) and isinstance(lst[cursor[1]], list) and len(shape) > 1:
                sub = [lst[cursor[1]], 0]
                cursor[1] += 1
                out.append(take_all(sub, shape[1:]))
            elif cursor[1] < len(lst) and isinstance(lst[cursor[1]], list):
                sub = lst[cursor[1]]
                cursor[1] += 1
                out.append(sub[0] if sub else 0)
            else:
                out.append(take(cursor, shape[1:]))
        return out

    def take_all(cursor, shape):
        result = take(cursor, shape)
        if cursor[1] < len(cursor[0]):
            raise SystemExit(f"{name}: more initializers than {shape} holds")
        return result

    return take_all([items, 0], shape)


def unwrap_structs(values, shape):
    """`values` filled to `shape` with each `STRUCT` level read as one element,
    and those levels taken out again."""
    if not shape:
        return values
    if shape[0] == STRUCT:
        return unwrap_structs(values[0], shape[1:])
    return [unwrap_structs(v, shape[1:]) for v in values]


def rust_array(values, shape, ty, indent=0) -> str:
    if len(shape) == 1:
        nums = [str(v) for v in values]
        lines = []
        line = ""
        for n in nums:
            piece = n + ", "
            if len(line) + len(piece) > 92 - indent:
                lines.append(line.rstrip())
                line = ""
            line += piece
        if line:
            lines.append(line.rstrip())
        pad = " " * (indent + 4)
        return "[\n" + "\n".join(pad + l for l in lines) + "\n" + " " * indent + "]"
    inner = ",\n".join(" " * (indent + 4) + rust_array(v, shape[1:], ty, indent + 4)
                       for v in values)
    return "[\n" + inner + ",\n" + " " * indent + "]"


def rust_type(ty, shape):
    t = ty
    for n in reversed(shape):
        t = f"[{t}; {n}]"
    return t


def wrapped(values, ty):
    """`values` converted into `ty` as C converts an out-of-range constant into
    a signed element -- modulo 2 to the element's width, as every compiler the
    C libraries are built with does: FFmpeg writes some `int32_t` tables in
    hex, `0xf383a3e3` among them."""
    if isinstance(values, list):
        return [wrapped(v, ty) for v in values]
    bits = {"i8": 8, "i16": 16, "i32": 32}[ty]
    v = values % (1 << bits)
    return v - (1 << bits) if v >= 1 << (bits - 1) else v


def check_range(name, values, ty):
    flat = values
    while flat and isinstance(flat[0], list):
        flat = [x for sub in flat for x in sub]
    lo, hi = {"u8": (0, 255), "i8": (-128, 127), "i16": (-32768, 32767), "u16": (0, 65535),
              "i32": (-2**31, 2**31 - 1), "u32": (0, 2**32 - 1), "u64": (0, 2**64 - 1)}[ty]
    for v in flat:
        if not lo <= v <= hi:
            raise SystemExit(f"{name}: {v} does not fit {ty}")


def refs_named(values, aliases):
    """A table of `CName`s as Rust references to the tables they name, each
    name first through `aliases` (a C macro naming one table by another);
    `NULL` as an empty slice."""
    if isinstance(values, list):
        return [refs_named(v, aliases) for v in values]
    name = aliases.get(values.name, values.name)
    return "&[]" if name == "NULL" else "&" + name.upper()


def q_values(values, frac_bits):
    """A float table's values through FFmpeg's `Q30` or `Q31`."""
    if isinstance(values, list):
        return [q_values(v, frac_bits) for v in values]
    return q_format(values, frac_bits)


def generate(root: str, tables, symbols: dict, header, source: str = "libvpx",
             item: str = "pub const", aliases: dict | None = None) -> bytes:
    """The Rust source for `tables`, each a tuple (C file under `root`, C name,
    Rust name, element type, shape, what it is), after the `//!` lines
    `header`: formatted by rustfmt, with LF line ends. `source` names the C
    library in each table's doc comment; `item` is how each table is declared
    (`pub(crate) static` for tables too big to copy into every use).

    A shape's first size may be `None`, for an array C sizes by its
    initializer. An element type ending in `%` is a signed one whose values are
    converted into it as C converts them (see `wrapped`); `q30` and `q31` are a
    C `float` array converted as FFmpeg's `Q30` and `Q31` convert its elements
    (see `q_format`), into `i32`; and `&[T]` is a table of pointers to other
    tables, each named in the initializer by its C name -- or by a macro in
    `aliases` that names one -- and written as a reference to the Rust table
    of that name upper-cased, which must be among `tables`; `NULL` an empty
    slice."""
    sources = {}
    out = list(header)
    for path, cname, rname, ty, shape, doc in tables:
        if path not in sources:
            with open(f"{root}/{path}", encoding="utf-8") as f:
                sources[path] = strip_comments(f.read())
        refs = ty.startswith("&")
        items = parse(initializer(sources[path], cname), symbols, keep_names=refs)
        if shape and shape[0] is None:
            shape = [len(items)] + list(shape[1:])
        c_shape = [1 if n == STRUCT else n for n in shape]
        values = unwrap_structs(fill(items, c_shape, cname), shape)
        rust_shape = [n for n in shape if n != STRUCT]
        if refs:
            names = values
            while names and isinstance(names[0], list):
                names = [x for sub in names for x in sub]
            if not all(isinstance(v, CName) for v in names):
                raise SystemExit(f"{cname}: a table of pointers holds something else")
            values = refs_named(values, aliases or {})
        elif ty in ("q30", "q31"):
            values = q_values(values, int(ty[1:]))
            ty = "i32"
        elif ty.endswith("%"):
            ty = ty[:-1]
            values = wrapped(values, ty)
        if not refs:
            check_range(cname, values, ty)
        if doc:
            out.append(f"/// {doc[0].upper() + doc[1:]}: {source}'s `{cname}`.")
        else:
            out.append(f"/// {source}'s `{cname}`.")
        # A reference in a static's or a constant's type is 'static already.
        rust_ty = rust_type(ty, rust_shape)
        out.append(f"{item} {rname}: {rust_ty} = "
                   f"{rust_array(values, rust_shape, ty)};")
        out.append("")
    # Through rustfmt, so the file is formatted the way every Rust file in the
    # tree is (the pre-push gate formats each file on its own, and allows no
    # exceptions), and so that regenerating and diffing still compares like
    # with like. Bytes, not text: on Windows a text-mode stdout turns every
    # newline into CRLF.
    formatted = subprocess.run(
        ["rustfmt", "--edition", "2024", "--emit", "stdout"],
        input="\n".join(out).encode("utf-8"),
        capture_output=True,
        check=True,
    ).stdout
    return formatted.replace(b"\r\n", b"\n")

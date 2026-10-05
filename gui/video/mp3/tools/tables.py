"""Convert minimp3's static tables to Rust, from the C source, so that no
number is copied by hand. Prints src/tables.rs:

    python tools/tables.py <minimp3>/minimp3.h halfrate g_deq_L12 \\
        g_bitalloc_code_tab g_scf_long g_scf_short g_scf_mixed \\
        g_scf_partitions g_scfc_decode g_mod g_preamp g_expfrac g_pow43 \\
        tabs tab32 tab33 tabindex g_linbits g_pan g_aa g_twid9 g_twid3 \\
        g_mdct_window g_sec g_win > src/tables.rs
    cargo fmt -p mp3

Nested arrays keep their rows: each row is padded with zeros to its declared
length, as C pads a short initializer. `g_deq_L12`, written in C through a
macro of three divisions, is written as the same divisions, which Rust
evaluates in f32 as C's compiler does."""
import re
import sys

src = open(sys.argv[1], encoding="utf-8").read()


def braces(text, i):
    """The text inside the braces opening at `i`, and where they close."""
    depth = 0
    for j in range(i, len(text)):
        if text[j] == "{":
            depth += 1
        elif text[j] == "}":
            depth -= 1
            if depth == 0:
                return text[i + 1:j], j
    sys.exit("unbalanced")


def parse(body, dims):
    """The values of an initializer of shape `dims`, padded and flattened."""
    body = re.sub(r"/\*.*?\*/", "", body, flags=re.S)
    if len(dims) == 1:
        values = [v.strip() for v in body.split(",") if v.strip()]
        if dims[0] is not None:
            if len(values) > dims[0]:
                sys.exit("too many values")
            values += ["0"] * (dims[0] - len(values))
        return values
    out = []
    i = 0
    rows = 0
    while True:
        k = body.find("{", i)
        if k < 0:
            break
        inner, end = braces(body, k)
        out += parse(inner, dims[1:])
        rows += 1
        i = end + 1
    if dims[0] is not None:
        size = 1
        for d in dims[1:]:
            size *= d
        out += ["0"] * ((dims[0] - rows) * size)
    return out


def table(name):
    m = re.search(r"static const (\w+) " + re.escape(name) + r"((?:\[[^\]]*\])+)\s*=\s*\{", src)
    if not m:
        sys.exit(f"no table {name}")
    ctype = m.group(1)
    dims = [eval(d) if d.strip() else None for d in re.findall(r"\[([^\]]*)\]", m.group(2))]
    body, _ = braces(src, m.end() - 1)
    return ctype, dims, parse(body, dims)


RUST = {"uint8_t": "u8", "int16_t": "i16", "float": "f32", "unsigned": "u32", "int": "i32", "uint16_t": "u16"}


def literal(v, rtype):
    if rtype == "f32":
        v = v.rstrip("f")
        if "." not in v and "e" not in v.lower():
            v += ".0"
    return v


out = ["//! minimp3's tables, converted from its C source by `tools/tables.py`, not",
       "//! copied by hand (minimp3, CC0: `licenses/minimp3-LICENSE`).",
       "",
       "#![allow(clippy::unreadable_literal, clippy::excessive_precision, reason = \"minimp3's own literals, as written\")]",
       ""]
for name in sys.argv[2:]:
    rname = name.upper()
    if name == "g_deq_L12":
        divisors = [3, 7, 15, 31, 63, 127, 255, 511, 1023, 2047, 4095, 8191, 16383, 32767, 65535, 3, 5, 9]
        vals = ", ".join(f"{a}_f32 / {d}.0" for d in divisors for a in ("9.53674316e-07", "7.56931807e-07", "6.00777173e-07"))
        out.append("/// minimp3's `g_deq_L12`[18*3]: its macro's divisions.")
        out.append(f"pub(crate) static {rname}: [f32; 54] = [{vals}];")
        out.append("")
        continue
    ctype, dims, values = table(name)
    rtype = RUST[ctype]
    vals = ", ".join(literal(v, rtype) for v in values)
    shape = "".join(f"[{d}]" if d is not None else "[]" for d in dims)
    out.append(f"/// minimp3's `{name}`{shape}, flattened.")
    out.append(f"pub(crate) static {rname}: [{rtype}; {len(values)}] = [{vals}];")
    out.append("")
print("\n".join(out))

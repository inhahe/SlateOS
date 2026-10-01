"""The directed-rounding oracles, shared: run an oracle harness's C program
once in each rounding mode and keep the calls whose answers differ from
their answers to nearest.

    write_modes_oracle(gen_c(), NEAREST, OUT, "what the calls are")

`gen_c()` is a harness's program: `int main(void) { ... return 0; }`,
printing one call a line, `<name> <in...> = <out...> <errno>`. Its calls run
under FE_TONEAREST, then FE_DOWNWARD, FE_UPWARD and FE_TOWARDZERO. The
to-nearest pass must be `NEAREST` -- the harness's own table -- line for
line, or the two tables would describe different programs, and nothing is
written. `OUT` gets `<mode> <call>`, mode `down`, `up` or `zero`, for each
directed call `keep(near, directed)` accepts; a call it does not list
answers as `NEAREST` says (or, with a narrower `keep`, as the test that
reads it says).

Used by math_modes_harness.py and mathl_modes_harness.py, which keep every
call that differs, and complex_modes_harness.py and
complexl_modes_harness.py, which keep only the calls whose answer changes
in kind ([`changes_kind`]): theirs are inexact nearly everywhere, and the
full tables would be 4.7 and 4.9 MB.
"""

import sys
from pathlib import Path

from _wsl import run, workdir, wsl_path

MODES = [("near", "FE_TONEAREST"), ("down", "FE_DOWNWARD"), ("up", "FE_UPWARD"),
         ("zero", "FE_TOWARDZERO")]


def differs(near, directed):
    """Keep every call whose answer differs at all."""
    return near != directed


def kind(token):
    """What a printed value is: ('nan',), ('inf', sign), ('zero', sign),
    ('finite', sign) -- or the token itself for an integer. Doubles and
    floats are 16 and 8 hex digits, long doubles `SSSS:MMMMMMMMMMMMMMMM`."""
    if ":" in token:
        se, m = (int(t, 16) for t in token.split(":"))
        sign, e = se >> 15, se & 0x7FFF
        top, frac, zero = 0x7FFF, m & ~(1 << 63), m == 0
    elif len(token) in (8, 16):
        b = int(token, 16)
        width = 4 * len(token)
        ebits = 11 if width == 64 else 8
        sign = b >> (width - 1)
        e = (b >> (width - 1 - ebits)) & ((1 << ebits) - 1)
        frac = b & ((1 << (width - 1 - ebits)) - 1)
        top, zero = (1 << ebits) - 1, e == 0 and frac == 0
    else:
        return (token,)
    if e == top:
        return ("nan",) if frac else ("inf", sign)
    if zero:
        return ("zero", sign)
    return ("finite", sign)


def changes_kind(near, directed):
    """Keep a call whose answer changes in kind -- an infinity, a NaN or a
    zero where to nearest it is not, or of the other sign -- or in errno."""
    a = near.split(" = ")[1].split(" ")
    b = directed.split(" = ")[1].split(" ")
    return a[-1] != b[-1] or [kind(t) for t in a[:-1]] != [kind(t) for t in b[:-1]]


def in_every_mode(c):
    """A harness's program with its calls run once in each mode."""
    assert c.count("int main(void) {") == 1
    c = c.replace("int main(void) {", "static void run_all(void) {")
    tail = "  return 0;\n}\n"
    assert c.endswith(tail)
    c = c[: -len(tail)] + "}\n"
    # Beside the harness's own first #include: after its `#define
    # _GNU_SOURCE`, which must come before any header for `exp10` and the
    # other GNU functions to be declared.
    first = c.index("#include")
    c = c[:first] + "#include <fenv.h>\n" + c[first:]
    c += "int main(void) {\n"
    for tag, mode in MODES:
        c += ('  printf("@' + tag + '\\n"); fflush(stdout); fesetround(' + mode
              + '); run_all(); fflush(stdout); fesetround(FE_TONEAREST);\n')
    c += "  return 0;\n}\n"
    return c


def calls(path):
    """The call lines of an oracle table, comments and blanks dropped."""
    return [line for line in Path(path).read_text(encoding="utf-8").splitlines()
            if line and not line.startswith("#")]


def write_modes_oracle(c, nearest, out, what, keep=differs):
    """Run program `c` in every mode; write `out` as the module says."""
    with workdir() as tmp:
        (Path(tmp) / "modes.c").write_text(in_every_mode(c), encoding="utf-8", newline="\n")
        r = run(f"cd {wsl_path(tmp)} && gcc -O0 -fno-builtin -frounding-math -w -o modes "
                "modes.c -lm && ./modes")
    if r.returncode != 0:
        sys.exit(f"oracle failed:\n{r.stdout[-2000:]}\n{r.stderr[-4000:]}")
    by_mode, tag = {t: [] for t, _ in MODES}, None
    for line in r.stdout.splitlines():
        if not line.strip():
            continue
        if line.startswith("@"):
            tag = line[1:]
            continue
        by_mode[tag].append(line)
    near = calls(nearest)
    if by_mode["near"] != near:
        sys.exit(f"{Path(nearest).name} is not this program's round-to-nearest answers: "
                 "run its harness first")
    rows, differing = [], 0
    for t, _ in MODES[1:]:
        assert len(by_mode[t]) == len(near), (t, len(by_mode[t]), len(near))
        for n, directed in zip(near, by_mode[t]):
            assert n.split(" = ")[0] == directed.split(" = ")[0], (n, directed)
            differing += directed != n
            if directed != n and keep(n, directed):
                rows.append(f"{t} {directed}")
    nm = Path(nearest).name
    if keep is differs:
        rule = f"# differs from the one {nm} gives to nearest.\n"
    else:
        rule = (f"# differs in kind -- an infinity, a NaN or a zero where {nm} has none, or\n"
                f"# of the other sign -- or in errno from the one {nm} gives to nearest.\n")
    header = (f"# glibc 2.39 under WSL in the directed rounding modes: {what}\n"
              f"# (posix/tools/oracle/_modes.py). <mode> <call>, for each call whose answer\n"
              + rule)
    Path(out).write_text(header + "\n".join(rows) + "\n", encoding="utf-8", newline="\n")
    print(f"{len(rows)} of {3 * len(near)} directed calls kept ({differing} differ from "
          f"nearest) -> {out}")

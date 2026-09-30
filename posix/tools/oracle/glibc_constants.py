"""glibc 2.39's and Linux's values for the constants posix defines, as the
reference scripts/check-libc-abi.py holds the ones musl's headers lack to.

    python posix/tools/oracle/glibc_constants.py   # writes glibc_constants.txt

check-libc-abi.py compares each of the library's public constants with the
header a C program here compiles against, musl's. A constant no musl header
defines -- glibc's own (`RES_NOTLDQUERY`, `RTLD_DI_ORIGIN`), or the kernel's
(`BPF_*`, `IORING_OP_*`) -- had no oracle at all until 2026-09-30, and one of
them was wrong: `RES_NOTLDQUERY` was `RES_USE_EDNS0`'s bit. This asks the
headers of a glibc system -- Ubuntu 24.04's, under WSL: glibc 2.39's
(`dpkg -L libc6-dev`) and the kernel's (`dpkg -L linux-libc-dev`) -- for the
value each of the library's constant names has there.

How: every header, each alone, in a translation unit of its own (they do not
all compile together): a header that does not compile alone is passed over.
In each, a probe line per name, `enum { p = sizeof(char[(NAME) ? 1 : 1]) };`,
compiles only where NAME is an integer constant expression -- a macro or an
enum constant -- and fails where it is anything else (undeclared, a type, a
function, a string); the names that compiled are printed by a program built
from the same header: the value, and the width of its C type, for
comparing "in the bits both have" as the musl half does.

The table (glibc_constants.txt): per name, each distinct value a glibc
header gives it (`libc`), else each a kernel header does (`kernel`), with
the first header that says so -- or `-`, looked up and defined by neither.
The gate uses it only for names musl's headers do not answer for, and
refuses a library constant the table has not looked up: regenerate.
"""

import importlib.util
import json
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
from _wsl import run, workdir, wsl_path  # noqa: E402

ROOT = HERE.parent.parent.parent
OUT = HERE / "glibc_constants.txt"

_spec = importlib.util.spec_from_file_location("abi", ROOT / "scripts" / "check-libc-abi.py")
abi = importlib.util.module_from_spec(_spec)
sys.modules["abi"] = abi
_spec.loader.exec_module(abi)

PROBER = r'''
import concurrent.futures, json, os, re, subprocess, sys, tempfile

names = open(sys.argv[1]).read().split()
headers = [l.split() for l in open(sys.argv[2]) if l.strip()]   # side, header
FLAGS = ["-std=gnu17", "-w", "-fmax-errors=0"]
AT = re.compile(r"t\.c:(\d+):\d+: (?:fatal )?(?:error|note: in expansion of macro)")

def gcc(src, run=False):
    with tempfile.TemporaryDirectory() as d:
        c = os.path.join(d, "t.c")
        with open(c, "w") as f:
            f.write(src)
        if not run:
            r = subprocess.run(["gcc", *FLAGS, "-fsyntax-only", c], capture_output=True, text=True)
            return r, None
        r = subprocess.run(["gcc", *FLAGS, "-o", os.path.join(d, "t"), c], capture_output=True, text=True)
        if r.returncode != 0:
            return r, None
        p = subprocess.run([os.path.join(d, "t")], capture_output=True, text=True)
        return r, p.stdout

def preprocessed(src):
    """`gcc -E -dD`: the unit as the compiler sees it, and its #defines."""
    with tempfile.TemporaryDirectory() as d:
        c = os.path.join(d, "t.c")
        with open(c, "w") as f:
            f.write(src)
        return subprocess.run(["gcc", "-std=gnu17", "-w", "-E", "-dD", c],
                              capture_output=True, text=True)

WORD = re.compile(r"[A-Za-z_][A-Za-z0-9_]*")

def one(item):
    side, h = item
    base = "#define _GNU_SOURCE 1\n#include <%s>\n" % h
    r, _ = gcc(base)
    if r.returncode != 0:
        return side, h, None
    # Only a name the header's text holds can be one of its constants: the
    # rest would each be an undeclared identifier, which gcc answers with a
    # search for a near spelling -- seconds a unit, thousands of them.
    words = set(WORD.findall(preprocessed(base).stdout))
    cands = [n for n in names if n in words]
    first = base.count("\n") + 1
    src = base + "".join("enum { slate_p%d = sizeof(char[(%s) ? 1 : 1]) };\n" % (i, n)
                         for i, n in enumerate(cands))
    r, _ = gcc(src)
    bad = {int(m.group(1)) for m in AT.finditer(r.stderr)}
    ok = [n for i, n in enumerate(cands) if first + i not in bad]
    for _attempt in range(8):
        if not ok:
            return side, h, {}
        top = base + "int printf(const char *, ...);\nint main(void) {\n"
        first = top.count("\n") + 1
        body = "".join('  printf("%%s %%d %%d %%lld %%llu\\n", "%s", (int)sizeof(%s), (%s) < 0, '
                       '(long long)(%s), (unsigned long long)(%s));\n' % (n, n, n, n, n) for n in ok)
        r, out = gcc(top + body + "  return 0;\n}\n", run=True)
        if out is not None:
            vals = {}
            for line in out.splitlines():
                n, size, neg, s, u = line.split()
                vals[n] = [int(s) if neg == "1" else int(u), int(size) * 8]
            return side, h, vals
        bad = {int(m.group(1)) for m in AT.finditer(r.stderr)}
        if not bad:
            return side, h, {"?": r.stderr[-1500:]}
        ok = [n for i, n in enumerate(ok) if first + i not in bad]
    return side, h, {"?": "did not settle"}

with concurrent.futures.ThreadPoolExecutor(max_workers=os.cpu_count() or 4) as ex:
    results = list(ex.map(one, headers))
json.dump(results, open(sys.argv[3], "w"))
'''


def headers_of(package: str) -> list[str]:
    r = run(f"dpkg -L {package}")
    if r.returncode != 0:
        sys.exit(f"dpkg -L {package} failed:\n{r.stderr}")
    out = []
    for line in r.stdout.splitlines():
        if line.startswith("/usr/include/") and line.endswith(".h"):
            rel = line[len("/usr/include/"):]
            # multiarch: <sys/types.h> lives in x86_64-linux-gnu/sys/
            if rel.startswith("x86_64-linux-gnu/"):
                rel = rel[len("x86_64-linux-gnu/"):]
            if "/bits/" in "/" + rel or rel.startswith("gnu/stubs"):
                continue
            out.append(rel)
    return sorted(set(out))


def main() -> None:
    doc, err = abi.run_rustdoc()
    if doc is None:
        sys.exit(f"rustdoc: {err}")
    consts, _unread = abi.constants_from_rustdoc(doc)
    names = sorted({c.name for c in consts})
    libc = headers_of("libc6-dev")
    kernel = [h for h in headers_of("linux-libc-dev") if h not in set(libc)]
    ver = run("ldd --version | head -1").stdout.strip()
    kver = run("dpkg-query -W -f '${Version}' linux-libc-dev").stdout.strip()
    with workdir() as t:
        d = Path(t)
        (d / "names.txt").write_text("\n".join(names) + "\n", encoding="utf-8", newline="\n")
        (d / "headers.txt").write_text("".join(f"libc {h}\n" for h in libc)
                                       + "".join(f"kernel {h}\n" for h in kernel),
                                       encoding="utf-8", newline="\n")
        (d / "prober.py").write_text(PROBER, encoding="utf-8", newline="\n")
        r = run(f"python3 {wsl_path(d / 'prober.py')} {wsl_path(d / 'names.txt')} "
                f"{wsl_path(d / 'headers.txt')} {wsl_path(d / 'out.json')}")
        if r.returncode != 0:
            sys.exit(f"the prober failed:\n{r.stderr[-3000:]}")
        results = json.loads((d / "out.json").read_text(encoding="utf-8"))
    table: dict[str, dict[str, dict[tuple[int, int], str]]] = {}
    unusable, confused = [], []
    for side, h, vals in results:
        if vals is None:
            unusable.append(h)
            continue
        if "?" in vals:
            confused.append(h)
            continue
        for n, (v, bits) in vals.items():
            table.setdefault(n, {}).setdefault(side, {}).setdefault((v, bits), h)
    lines = [
        "# glibc's and Linux's values for the names of posix's public constants: per name,",
        "# each distinct value a glibc header gives it (libc), else each a kernel header does",
        "# (kernel) -- value, the width in bits of its C type, the first header -- or `-` for a",
        "# name neither defines as an integer constant. scripts/check-libc-abi.py compares the",
        "# library with it for the names musl's headers do not define.",
        f"# From {ver} and linux-libc-dev {kver} (Ubuntu, WSL), each header alone; "
        f"{len(libc)} glibc and {len(kernel)} kernel headers, {len(unusable)} of which do not "
        "compile alone.",
        "# Generated by posix/tools/oracle/glibc_constants.py; do not edit.",
    ]
    for n in names:
        sides = table.get(n, {})
        side = "libc" if "libc" in sides else "kernel" if "kernel" in sides else None
        if side is None:
            lines.append(f"{n}\t-")
            continue
        for (v, bits), h in sorted(sides[side].items()):
            lines.append(f"{n}\t{side}\t{v}\t{bits}\t{h}")
    OUT.write_text("\n".join(lines) + "\n", encoding="utf-8", newline="\n")
    found = sum(1 for n in names if n in table)
    print(f"{OUT.name}: {len(names)} names, {found} defined by a glibc or kernel header; "
          f"{len(unusable)} headers do not compile alone, {len(confused)} would not settle"
          + (f": {' '.join(confused[:8])}" if confused else ""))


if __name__ == "__main__":
    main()

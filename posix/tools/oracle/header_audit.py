"""Where the headers C is compiled with here -- musl's, with posix/include in
front of them -- declare the names libc.a defines under other feature macros
than glibc 2.39's headers do. A report, not a gate: run it after zig brings new
musl headers, or when a port written for glibc does not compile.

    python posix/tools/oracle/header_audit.py [--all]

For each header both C libraries have, and each of
scripts/check-libc-overlay.py's feature-macro settings (CONFIGS), it compares
which of libc.a's public names are declared: glibc's by libclang, counting a
header's *own* declarations -- in the header or the internal files it
includes (bits/, gnu/), not through another public header, since musl's
headers include far fewer of each other and a program leaning on glibc's
doing so is leaning on nothing a standard says -- and ours by compiling a
probe with zig. It prints each name either side hides where the other shows
it:

    <header> <name>: glibc <settings>; here <settings>

"hidden here" is a name a program written for glibc uses and does not find
-- the overlay's to add, under glibc's feature macros (design-decisions
§1141) -- unless it is one of the differences taken on purpose, which the
report marks: the LFS64 names, which musl gives only to _LARGEFILE64_SOURCE.
"shown here" is a name a strictly conforming program might define itself;
printed with --all.

Needs WSL with glibc 2.39's headers and libclang's Python bindings, as
glibc_declarations.py does (its docstring says how), and zig (FASTPY_ZIG or
PATH).
"""

import concurrent.futures
import importlib.util
import json
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
from _wsl import run, workdir, wsl_path  # noqa: E402
from glibc_declarations import LIBCLANG, PYTHON  # noqa: E402

ROOT = HERE.parent.parent.parent


def load(name: str, path: Path):
    spec = importlib.util.spec_from_file_location(name, path)
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


overlay = load("check_libc_overlay", ROOT / "scripts" / "check-libc-overlay.py")
declared = load("check_libc_declared", ROOT / "scripts" / "check-libc-declared.py")

# Run in WSL: argv = names file, headers file, configurations file, output.
READER = r'''
import json, sys
import clang.cindex as ci
ci.Config.set_library_file(LIBCLANG)
names = set(open(sys.argv[1]).read().split())
headers = open(sys.argv[2]).read().split()
configs = json.load(open(sys.argv[3]))
idx = ci.Index.create()

def internal(path):
    return "/bits/" in path or "/gnu/" in path or path.endswith(
        ("/sys/cdefs.h", "/features.h", "/features-time64.h", "/stdc-predef.h"))

out = {}
for h in headers:
    per = out.setdefault(h, {})
    for cfg, flags in configs.items():
        tu = idx.parse("t.c", args=["-x", "c", *flags], unsaved_files=[("t.c", "#include <%s>\n" % h)])
        parent = {}
        for inc in tu.get_includes():
            if inc.include is not None and inc.include.name not in parent:
                parent[inc.include.name] = inc.source.name if inc.source else None
        top = next((f for f, p in parent.items() if p == "t.c"), None)

        def own(f):
            x, n = f, 0
            while x is not None and x != top and n < 100:
                if not internal(x):
                    return False
                x, n = parent.get(x), n + 1
            return x == top

        for c in tu.cursor.get_children():
            if c.kind not in (ci.CursorKind.FUNCTION_DECL, ci.CursorKind.VAR_DECL):
                continue
            if c.spelling in names and c.location.file is not None and own(c.location.file.name):
                cfgs = per.setdefault(c.spelling, [])
                if cfg not in cfgs:
                    cfgs.append(cfg)
json.dump(out, open(sys.argv[4], "w"))
'''.replace("LIBCLANG", repr(LIBCLANG))

LFS64 = "LFS64: musl's headers give it only to _LARGEFILE64_SOURCE, on purpose"


def ours(zig: str, header: str, names: list[str]) -> dict[str, set[str]]:
    """name -> the settings in which <header>, with the overlay in front,
    declares it: as a function, an object or a macro."""
    out = {n: set() for n in names}
    body = "".join(f"#ifndef {n}\n(void)&{n};\n#endif\n" for n in names)
    for cfg, flags in overlay.CONFIGS.items():
        src = f"#include <{header}>\nvoid probe(void) {{\n{body}}}\n"
        _, diag = overlay.compile_c(zig, src, flags + ["-w", "-ferror-limit=0"], overlay.OVERLAY)
        missing = {m.group(1) for line in overlay.errors_of(diag)
                   for m in [overlay.UNDECLARED.search(line) or overlay.TYPE_NAME.search(line)] if m}
        for n in names:
            if n not in missing:
                out[n].add(cfg)
    return out


def main() -> None:
    show_all = "--all" in sys.argv
    zig = overlay.find_zig()
    musl = overlay.musl_include(zig) if zig else None
    if musl is None:
        sys.exit("needs zig (FASTPY_ZIG or PATH)")
    shape = declared.shape_module()
    public = declared.public_names(shape.parse_symbol_index(
        ROOT / "toolchain" / "sysroot" / "lib" / "libc.a"))
    heads = sorted({p.relative_to(b).as_posix() for b in (musl, overlay.OVERLAY)
                    for p in b.rglob("*.h") if not p.relative_to(b).as_posix().startswith("bits/")})
    with workdir() as t:
        d = Path(t)
        (d / "heads.txt").write_text("\n".join(heads) + "\n", encoding="utf-8", newline="\n")
        r = run(f"cd /usr/include && while read h; do [ -f \"$h\" ] || "
                f"[ -f \"x86_64-linux-gnu/$h\" ] && echo \"$h\"; done < {wsl_path(d / 'heads.txt')}")
        both = r.stdout.split()
        (d / "both.txt").write_text("\n".join(both) + "\n", encoding="utf-8", newline="\n")
        (d / "names.txt").write_text("\n".join(sorted(public)) + "\n", encoding="utf-8",
                                     newline="\n")
        (d / "configs.json").write_text(json.dumps(overlay.CONFIGS), encoding="utf-8",
                                        newline="\n")
        (d / "reader.py").write_text(READER, encoding="utf-8", newline="\n")
        r = run(f"{PYTHON} {wsl_path(d / 'reader.py')} {wsl_path(d / 'names.txt')} "
                f"{wsl_path(d / 'both.txt')} {wsl_path(d / 'configs.json')} "
                f"{wsl_path(d / 'glibc.json')}")
        if r.returncode != 0:
            sys.exit(f"the reader failed:\n{r.stderr}")
        glibc = json.loads((d / "glibc.json").read_text(encoding="utf-8"))
    order = list(overlay.CONFIGS)

    def fmt(cfgs):
        return ",".join(sorted(cfgs, key=order.index)) or "-"

    with concurrent.futures.ThreadPoolExecutor(max_workers=16) as pool:
        here = dict(zip(both, pool.map(lambda h: ours(zig, h, sorted(glibc.get(h, {}))), both)))
    hidden = shown = 0
    for h in both:
        for n, gc in sorted(glibc.get(h, {}).items()):
            g, o = set(gc), here[h].get(n, set())
            if g - o:
                hidden += 1
                why = f"  ({LFS64})" if n.endswith("64") and g - o == {"gnu"} else ""
                print(f"<{h}> {n}: glibc {fmt(g)}; here {fmt(o)} -- hidden here{why}")
            elif o - g and show_all:
                print(f"<{h}> {n}: glibc {fmt(g)}; here {fmt(o)} -- shown here")
            shown += bool(o - g)
    print(f"{len(both)} headers both have; {sum(len(v) for v in glibc.values())} of glibc's own "
          f"declarations of libc.a's names; {hidden} hidden here somewhere glibc declares them, "
          f"{shown} declared here somewhere glibc does not" + ("" if show_all else " (--all lists them)"))


if __name__ == "__main__":
    main()

"""glibc 2.39's declarations of the names posix/include declares, as the
reference scripts/check-libc-overlay.py holds the overlay to.

    python posix/tools/oracle/glibc_declarations.py   # writes glibc_declarations.txt

For each name the overlay adds to musl's headers: the overlay header that
declares it, the feature-macro configurations (check-libc-overlay.py's
CONFIGS) in which glibc's header of that name declares it, and its type as
glibc declares it, typedefs resolved -- all read out of glibc's headers by
clang, through libclang's Python bindings, in WSL.

One line a name, tab-separated:

    <name> <header> <configurations, comma-separated, or -> <type>

The bindings are not in Ubuntu's base install. Once:

    python3 -m venv ~/.cache/slateos-libclang
    ~/.cache/slateos-libclang/bin/pip install libclang==18.1.1

(or set SLATEOS_LIBCLANG_PYTHON to a Python that has them). They drive the
system's libclang-18 -- the one Ubuntu's clang package installs -- so that
clang finds its own headers.
"""

import importlib.util
import os
import re
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
from _wsl import run, workdir, wsl_path  # noqa: E402

ROOT = HERE.parent.parent.parent
OUT = HERE / "glibc_declarations.txt"
PYTHON = os.environ.get("SLATEOS_LIBCLANG_PYTHON", "~/.cache/slateos-libclang/bin/python")
LIBCLANG = "/usr/lib/llvm-18/lib/libclang-18.so.1"

_spec = importlib.util.spec_from_file_location("overlay", ROOT / "scripts" / "check-libc-overlay.py")
overlay = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(overlay)

# Run in WSL: argv = names file, headers file, configurations file, output.
READER = r'''
import json, sys
import clang.cindex as ci
ci.Config.set_library_file(LIBCLANG)
names = set(open(sys.argv[1]).read().split())
headers = open(sys.argv[2]).read().split()
configs = json.load(open(sys.argv[3]))
idx = ci.Index.create()
out = {}
for h in headers:
    for cfg, flags in configs.items():
        tu = idx.parse("t.c", args=["-x", "c", *flags], unsaved_files=[("t.c", "#include <%s>\n" % h)])
        for c in tu.cursor.get_children():
            if c.kind in (ci.CursorKind.FUNCTION_DECL, ci.CursorKind.VAR_DECL) and c.spelling in names:
                e = out.setdefault(c.spelling, {"type": c.type.get_canonical().spelling, "in": {}})
                e["in"].setdefault(h, [])
                if cfg not in e["in"][h]:
                    e["in"][h].append(cfg)
json.dump(out, open(sys.argv[4], "w"))
'''.replace("LIBCLANG", repr(LIBCLANG))


def declaring_header(name: str, texts: dict[str, str]) -> str:
    """The overlay header that declares `name` (its text, comments and
    preprocessor lines aside, has the name as a word)."""
    found = [h for h, t in texts.items() if re.search(r"\b" + re.escape(name) + r"\b", t)]
    if len(found) != 1:
        sys.exit(f"{name}: declared in {found or 'no overlay header'}; expected exactly one")
    return found[0]


def main() -> None:
    zig = overlay.find_zig()
    if zig is None or overlay.musl_include(zig) is None:
        sys.exit("needs zig (FASTPY_ZIG or PATH), to see what the overlay declares")
    names = sorted(overlay.overlay_names(zig, overlay.OVERLAY))
    texts = {}
    for h in overlay.overlay_headers(overlay.OVERLAY):
        t = re.sub(r"/\*.*?\*/", " ", (overlay.OVERLAY / h).read_text(encoding="utf-8"), flags=re.S)
        texts[h] = "\n".join(line for line in t.splitlines() if not line.lstrip().startswith("#"))
    where = {n: declaring_header(n, texts) for n in names}
    with workdir() as t:
        d = Path(t)
        (d / "names.txt").write_text("\n".join(names) + "\n", encoding="utf-8", newline="\n")
        (d / "headers.txt").write_text("\n".join(sorted(set(where.values()))) + "\n",
                                       encoding="utf-8", newline="\n")
        import json
        (d / "configs.json").write_text(json.dumps(overlay.CONFIGS), encoding="utf-8", newline="\n")
        (d / "reader.py").write_text(READER, encoding="utf-8", newline="\n")
        r = run(f"{PYTHON} {wsl_path(d / 'reader.py')} {wsl_path(d / 'names.txt')} "
                f"{wsl_path(d / 'headers.txt')} {wsl_path(d / 'configs.json')} "
                f"{wsl_path(d / 'out.json')}")
        if r.returncode != 0:
            sys.exit(f"the reader failed (see this file's docstring for its bindings):\n{r.stderr}")
        found = json.loads((d / "out.json").read_text(encoding="utf-8"))
        ver = run("ldd --version | head -1").stdout.strip()
    missing = [n for n in names if n not in found]
    if missing:
        sys.exit("glibc declares none of: " + " ".join(missing)
                 + "\n(an overlay declaration glibc does not have is not the overlay's to make)")
    order = list(overlay.CONFIGS)
    lines = [
        "# glibc 2.39's declarations of the names posix/include declares: the overlay header",
        "# declaring each, the configurations (scripts/check-libc-overlay.py's CONFIGS) glibc's",
        "# header of that name declares it in, and glibc's type for it, typedefs resolved.",
        f"# Generated by posix/tools/oracle/glibc_declarations.py from {ver}; do not edit.",
    ]
    for n in names:
        h = where[n]
        cfgs = sorted(found[n]["in"].get(h, []), key=order.index)
        lines.append(f"{n}\t{h}\t{','.join(cfgs) or '-'}\t{found[n]['type']}")
    OUT.write_text("\n".join(lines) + "\n", encoding="utf-8", newline="\n")
    print(f"{OUT.name}: {len(names)} declarations")


if __name__ == "__main__":
    main()

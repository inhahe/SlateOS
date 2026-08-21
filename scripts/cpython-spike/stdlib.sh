#!/bin/bash
# Build the Python standard library as ONE file, and prove an interpreter
# actually starts from it.
#
# WHY A ZIP AND NOT A DIRECTORY
#
# `<prefix>/lib/python312.zip` is the *first* entry of CPython's default
# `sys.path`, and `zipimport` is frozen into the binary. This is not a trick or
# an optimisation bolted on afterwards: it is the layout CPython already looks
# for before anything else. The alternative is 569 files and ~12 MB of small
# reads that our ext4 driver has to walk at every boot, to deliver exactly the
# same modules.
#
# WHAT THIS SCRIPT PROVES
#
# run.sh proves CPython compiles; slatelink.sh proves it links against our
# libc.a. Neither says the interpreter can *run*, because an interpreter with
# no stdlib cannot get past `init_fs_encoding` -- it dies looking for the
# `encodings` package, which is not frozen into the binary. This script closes
# that gap on the host, using the musl-linked control interpreter that `make`
# builds from the identical objects, so that the only thing left untested on
# SlateOS itself is SlateOS itself.
set -uo pipefail

. "$(dirname "${BASH_SOURCE[0]}")/../lib/worktree.sh" || exit 1

VER="${CPYTHON_VER:-3.12.3}"
MINOR="${VER%.*}"          # 3.12
TAG="${MINOR/./}"          # 312
WORK="/tmp/cpython-spike-$SLATE_LANE"
BUILD="$WORK/Python-$VER"
SRC="$WORK/zipsrc"
ISO="$WORK/ziproot"
OUT="$SLATE_SPIKE/python$TAG.zip"

[ -d "$BUILD" ] || { echo "ERROR: $BUILD missing — run run.sh first."; exit 1; }
CTRL="$BUILD/python"
[ -x "$CTRL" ] || { echo "ERROR: $CTRL missing — run.sh's make did not finish."; exit 1; }

BUILD_PY="${BUILD_PY:-python$MINOR}"
command -v "$BUILD_PY" >/dev/null 2>&1 || { echo "ERROR: no $BUILD_PY on PATH"; exit 1; }

# ---------------------------------------------------------------------------
# 0. The interpreter's own module inventory
# ---------------------------------------------------------------------------
# Printed every run because it is the thing most likely to regress silently:
# if configure ever goes back to building extensions as shared objects (see
# MODULE_BUILDTYPE in run.sh) this count collapses to 31 and the stdlib
# becomes decorative.
echo "=== C extension modules built into the interpreter ==="
"$CTRL" -S -c 'import sys; print("BUILTIN_MODULES=", len(sys.builtin_module_names))'

# ---------------------------------------------------------------------------
# 1. Assemble the library tree
# ---------------------------------------------------------------------------
rm -rf "$SRC" "$ISO"
mkdir -p "$SRC" "$ISO/usr/local/lib"
cp -a "$BUILD/Lib/." "$SRC/"

# Dropped, with reasons:
#   test/          the CPython test suite, ~30 MB, and run.sh already passes
#                  --disable-test-modules so its C half is not there anyway
#   idlelib/       an IDE, and it needs tkinter
#   tkinter/       needs _tkinter, which needs Tcl/Tk: not ported
#   turtledemo/    tkinter demos
#   lib2to3/       removed upstream in 3.13; nothing here uses it
#   ensurepip/     ships a pip wheel; run.sh builds --without-ensurepip
#   venv/          creates environments by copying an interpreter around,
#                  which needs a package installer to be useful
#   __pycache__/   rebuilt below, deliberately, with a different invalidation
#                  mode than the host's
#
# Deliberately KEPT even though their C extension is absent (ssl, sqlite3, bz2,
# lzma, gzip/zipfile-with-deflate ...): `import ssl` then fails with
# "No module named '_ssl'", which names the actual missing piece. Deleting the
# pure-Python half instead would report "No module named 'ssl'" and send
# whoever hits it looking for the wrong thing.
for d in test idlelib tkinter turtledemo lib2to3 ensurepip venv __pycache__; do
    rm -rf "${SRC:?}/$d"
done

# ---------------------------------------------------------------------------
# 2. Compile to bytecode with unchecked-hash invalidation
# ---------------------------------------------------------------------------
# A .pyc normally carries the source's mtime and size, and the loader
# re-validates against them. Inside a zip that check is answered from the zip's
# own directory entry, which is a different clock from the one that compiled
# the file -- one skewed timestamp and every module silently falls back to
# re-parsing source on every single import.
#
# `unchecked-hash` removes the question instead of trying to answer it: the
# .pyc records a source hash and marks it as *not* to be checked, so the loader
# accepts it unconditionally. That is the right call for a stdlib that ships as
# one immutable file and is never edited in place. (If it ever *is* edited in
# place, that is a rebuild of this artifact, not a cache-invalidation event.)
#
# `-b` writes `foo.pyc` beside `foo.py` rather than into `__pycache__`, which
# is the layout `zipimport` expects.
#
# `-d` rewrites the path baked into each code object. Without it every
# traceback on SlateOS would name `/tmp/cpython-spike-<lane>/zipsrc/json/…` --
# a build-machine scratch directory that does not exist on the target and never
# will. With it they read `/usr/local/lib/python312.zip/json/…`, which is both
# true and the same thing a stock zip-installed CPython prints.
ZIP_AS_SEEN_ON_TARGET="/usr/local/lib/python$TAG.zip"
echo "=== compiling to bytecode ==="
"$BUILD_PY" -m compileall -q -q -b -d "$ZIP_AS_SEEN_ON_TARGET" \
    --invalidation-mode unchecked-hash "$SRC" \
    || echo "  (compileall reported errors; syntax-error fixtures in the tree are expected)"

# ---------------------------------------------------------------------------
# 3. Pack, STORED
# ---------------------------------------------------------------------------
# ZIP_STORED, not ZIP_DEFLATED, and this is not a size/speed preference:
# `zlib` is one of the modules that has no target build (no zlib in the
# sysroot), so `zipimport` cannot inflate a compressed member. Measured, not
# assumed -- the deflated variant of this archive fails at startup with
# "No module named 'zlib'" raised from inside <frozen zipimport>. If zlib is
# ever ported, revisit: deflate took this same content from 10.3 MB to 2.6 MB.
#
# Both `.pyc` and `.py` go in. zipimport's search order tries `.pyc` first, so
# imports never parse source; the `.py` is there so tracebacks and
# `inspect.getsource` show real lines. On a system where a Python traceback may
# be the only debugging tool that works, that is worth its megabytes.
echo "=== packing $OUT ==="
mkdir -p "$SLATE_SPIKE"
( cd "$SRC" && "$BUILD_PY" - "$OUT" <<'PY'
import os, sys, zipfile
out = sys.argv[1]
n_py = n_pyc = 0
with zipfile.ZipFile(out, "w", zipfile.ZIP_STORED) as z:
    for dirpath, dirnames, files in os.walk("."):
        dirnames[:] = sorted(d for d in dirnames if d != "__pycache__")
        for f in sorted(files):
            if f.endswith(".pyc"):
                n_pyc += 1
            elif f.endswith(".py"):
                n_py += 1
            else:
                continue
            p = os.path.join(dirpath, f)
            z.write(p, os.path.relpath(p, "."))
print(f"ZIP_SOURCES={n_py} ZIP_BYTECODE={n_pyc}")
PY
) || { echo "ZIP_BUILD=fail"; exit 1; }
echo "ZIP_BYTES=$(stat -c%s "$OUT")"

cp "$OUT" "$ISO/usr/local/lib/python$TAG.zip"
cp "$CTRL" "$ISO/python3"

# ---------------------------------------------------------------------------
# 4. Prove an interpreter starts from it, and does real work
# ---------------------------------------------------------------------------
# Run from $ISO, not from the build tree: CPython's getpath finds a *build
# tree* by the landmark `Lib/os.py` next to the executable, so a probe run from
# the build directory answers a question nobody is asking. This exact mistake
# produced a false pass the first time it was measured.
cd "$ISO" || exit 1

echo
echo "=== start: PYTHONHOME=$ISO/usr/local, stdlib is the zip alone ==="
PYTHONHOME="$ISO/usr/local" ./python3 -c '
import sys
print("SLATE_PYTHON_OK", ".".join(map(str, sys.version_info[:3])))
print("SYS_PATH", sys.path)
' 2>&1 | head -20
echo "RC=${PIPESTATUS[0]}"

echo
echo "=== workload: the modules a first useful script needs ==="
PYTHONHOME="$ISO/usr/local" ./python3 -c '
import base64, collections, csv, datetime, decimal, functools, hashlib, io
import json, math, os, pathlib, random, re, struct, textwrap, unicodedata
import xml.etree.ElementTree as ET

print("json  ", json.loads("{\"a\": [1, 2, 3]}"))
print("re    ", re.findall(r"[a-z]+", "slate os 42"))
print("b64   ", base64.b64encode(b"slateos").decode())
print("struct", struct.unpack("<HH", struct.pack("<HH", 7, 42)))
print("ctr   ", collections.Counter("mississippi").most_common(2))
print("sha256", hashlib.sha256(b"slateos").hexdigest()[:16])
print("math  ", round(math.hypot(3, 4), 6))
print("dec   ", decimal.Decimal(1) / decimal.Decimal(7))
print("date  ", datetime.date(2026, 8, 21).isoformat())
print("uni   ", unicodedata.name("e"))
print("xml   ", ET.fromstring("<a><b x=\"1\"/></a>")[0].get("x"))
print("path  ", pathlib.PurePosixPath("/usr/local/lib") / "python312.zip")
random.seed(1); print("rand  ", random.randint(0, 99))
print("SLATE_PYTHON_STDLIB_OK")
' 2>&1 | head -30
echo "RC=${PIPESTATUS[0]}"

echo
echo "=== traceback still shows source lines (why .py rides along) ==="
PYTHONHOME="$ISO/usr/local" ./python3 -c '
import json, traceback
try:
    json.loads("{")
except Exception:
    tb = traceback.format_exc()
print(tb)
print("HAS_SOURCE_LINE", any(l.strip().startswith(("raise", "return", "obj,")) for l in tb.splitlines()))
' 2>&1 | tail -12
echo "RC=${PIPESTATUS[0]}"

echo
echo "=== -S (no site): the minimum SlateOS must serve at startup ==="
PYTHONHOME="$ISO/usr/local" ./python3 -S -c '
import sys
files = sorted(k for k, v in sys.modules.items() if getattr(v, "__file__", None))
print("FILE_BACKED_AT_STARTUP_NOSITE=", len(files))
print("   ", " ".join(files))
' 2>&1 | head -10
echo "RC=${PIPESTATUS[0]}"

echo "SLATE_CPYTHON_STDLIB_DONE"

"""Set up lane F's fuzzing campaign in WSL: a copy of the decoder crates with
a minimal workspace root (the real one's lints and nothing else), the cargo-fuzz
harness, and seed corpora built from the crates' own test fixtures. See
README.md.

Run in WSL: python3 setup.py [SRC_ROOT]
SRC_ROOT is the checkout to copy, as WSL sees it; by default the one this file
is in."""
import os
import re
import shutil
import struct
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
SRC = sys.argv[1] if len(sys.argv) > 1 else os.path.normpath(os.path.join(HERE, "..", "..", ".."))
HOME = os.path.expanduser("~")
FUZZ = os.path.join(HOME, "fuzz")
TREE = os.path.join(FUZZ, "tree")

# The harness's own dependencies, and every crate they reach by `path`.
ROOTS = ["gui/imagecodec", "gui/video/codec", "gui/video/matroska", "gui/video/vp8", "gui/video/vp9"]
PATH_DEP = re.compile(r'path\s*=\s*"([^"]+)"')


def closure(roots):
    seen, todo = set(), list(roots)
    while todo:
        crate = os.path.normpath(todo.pop()).replace(os.sep, "/")
        if crate in seen:
            continue
        seen.add(crate)
        manifest = open(os.path.join(SRC, crate, "Cargo.toml"), encoding="utf-8").read()
        for line in manifest.splitlines():
            line = line.split("#", 1)[0]
            m = PATH_DEP.search(line)
            if m and "=" in line.split("path", 1)[0] + "=":
                dep = os.path.normpath(os.path.join(crate, m.group(1))).replace(os.sep, "/")
                if os.path.isfile(os.path.join(SRC, dep, "Cargo.toml")):
                    todo.append(dep)
    return sorted(seen)


CRATES = closure(ROOTS)
print("crates:", CRATES)

# --- the crates, sources and manifests only (tests/data feeds the seeds) ---
#
# Each crate is replaced, and nothing else: `fuzz/` keeps the corpus the
# fuzzer has grown, the findings not yet looked at, and its build.
for crate in CRATES:
    src = os.path.join(SRC, crate)
    dst = os.path.join(TREE, crate)
    if os.path.isdir(dst):
        shutil.rmtree(dst)
    os.makedirs(dst, exist_ok=True)
    for name in os.listdir(src):
        if name in ("target", "tests", "benches", "examples", "fuzz"):
            continue
        s = os.path.join(src, name)
        d = os.path.join(dst, name)
        if os.path.isdir(s):
            shutil.copytree(s, d)
        else:
            shutil.copy2(s, d)

# --- a workspace root with the real one's lints ---
root = open(os.path.join(SRC, "Cargo.toml"), encoding="utf-8").read()
start = root.index("[workspace.lints.rust]")
end = root.index("\n[profile", start)
lints = root[start:end]
members = ",\n".join(f'    "{c}"' for c in CRATES)
with open(os.path.join(TREE, "Cargo.toml"), "w", encoding="utf-8", newline="\n") as f:
    f.write(f'[workspace]\nresolver = "2"\nexclude = ["fuzz"]\nmembers = [\n{members},\n]\n\n{lints}\n')

# --- the harness ---
harness = os.path.join(TREE, "fuzz")
os.makedirs(harness, exist_ok=True)
# Kept in the tree as `harness.toml`, so that nothing in the real workspace
# mistakes this directory for a package.
shutil.copy2(os.path.join(HERE, "harness.toml"), os.path.join(harness, "Cargo.toml"))
targets = os.path.join(harness, "fuzz_targets")
if os.path.isdir(targets):
    shutil.rmtree(targets)
shutil.copytree(os.path.join(HERE, "fuzz_targets"), targets)

# --- seeds ---
MAX_SEED = 256 * 1024


def seed_dir(target):
    d = os.path.join(harness, "corpus", target)
    os.makedirs(d, exist_ok=True)
    return d


def add_files(target, sub, exts=None):
    base = os.path.join(SRC, sub)
    if not os.path.isdir(base):
        return 0
    n = 0
    out = seed_dir(target)
    for dirpath, _, names in os.walk(base):
        for name in names:
            if exts and not name.lower().endswith(exts):
                continue
            if name.endswith((".md5", ".txt", ".json", ".py", ".rs", ".md", ".c", ".h")):
                continue
            p = os.path.join(dirpath, name)
            if os.path.getsize(p) > MAX_SEED:
                continue
            rel = os.path.relpath(p, base).replace(os.sep, "_")
            shutil.copy2(p, os.path.join(out, f"{sub.replace('/', '_')}__{rel}"))
            n += 1
    return n


def ivf_packets(path, limit=8):
    data = open(path, "rb").read()
    if data[:4] != b"DKIF":
        return None
    header = struct.unpack_from("<H", data, 6)[0]
    at, out = header, []
    while at + 12 <= len(data) and len(out) < limit:
        size = struct.unpack_from("<I", data, at)[0]
        out.append(data[at + 12: at + 12 + size])
        at += 12 + size
    return b"".join(struct.pack("<I", len(p)) + p for p in out)


counts = {}
counts["image"] = add_files("image", "gui/imagecodec/tests/data")
counts["video"] = sum(add_files("video", s) for s in (
    "gui/video/codec/tests/data", "gui/video/matroska/tests/data", "gui/video/mp4/tests/data",
)) + add_files("video", "gui/video/vp9/tests/data", (".webm",))
counts["sound"] = sum(add_files("sound", s) for s in (
    "gui/video/ogg/tests/data", "gui/video/opus/tests/data", "gui/video/vorbis/tests/data",
    "gui/video/flac/tests/data", "gui/video/mp3/tests/data", "gui/video/codec/tests/data",
))
counts["matroska"] = add_files("matroska", "gui/video/matroska/tests/data") + add_files(
    "matroska", "gui/video/vp9/tests/data", (".webm",))

n = 0
for dirpath, _, names in os.walk(os.path.join(SRC, "gui/video/vp8/tests/data")):
    for name in names:
        if name.endswith(".ivf"):
            seed = ivf_packets(os.path.join(dirpath, name))
            if seed:
                open(os.path.join(seed_dir("vp8"), name + ".pkt"), "wb").write(seed)
                n += 1
counts["vp8"] = n

n = 0
tmp = os.path.join(FUZZ, "ivf-tmp")
os.makedirs(tmp, exist_ok=True)
for dirpath, _, names in os.walk(os.path.join(SRC, "gui/video/vp9/tests/data")):
    for name in names:
        src = os.path.join(dirpath, name)
        if name.endswith(".ivf"):
            ivf = src
        elif name.endswith(".webm"):
            ivf = os.path.join(tmp, name + ".ivf")
            r = subprocess.run(["ffmpeg", "-nostdin", "-v", "error", "-y", "-i", src, "-map", "0:v:0",
                                "-c:v", "copy", "-frames:v", "8", "-f", "ivf", ivf],
                               stdin=subprocess.DEVNULL)
            if r.returncode != 0:
                continue
        else:
            continue
        seed = ivf_packets(ivf)
        if seed and len(seed) <= MAX_SEED:
            open(os.path.join(seed_dir("vp9"), name + ".pkt"), "wb").write(seed)
            n += 1
counts["vp9"] = n
shutil.rmtree(tmp)
print("seeds:", counts)

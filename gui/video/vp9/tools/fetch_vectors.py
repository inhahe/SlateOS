"""Fetch libvpx's VP9 conformance vectors and their per-frame MD5 files.

The VP9 decoder (gui/video/vp9) is checked against all 314 of libvpx's
vectors, the ones its own test/test_vectors.cc lists: each decoded picture's
MD5 must match the one libvpx publishes beside the vector. They total about
27 MB, too much to commit, so `tests/data` keeps a few hundred kilobytes of
them and this fetches the rest into `target/vp9vectors` at the workspace root,
where the full-suite tests look:

    python gui/video/vp9/tools/fetch_vectors.py
    cargo test -p vp9 --target x86_64-pc-windows-gnu -- --ignored

Files already present are not fetched again. The list is `vectors.txt`,
beside this script, copied from libvpx v1.17.0's test/test_vectors.cc.
"""
import concurrent.futures
import os
import shutil
import sys
import urllib.request

BASE = "https://storage.googleapis.com/downloads.webmproject.org/test_data/libvpx/"
HERE = os.path.dirname(os.path.abspath(__file__))
# gui/video/vp9/tools -> the workspace root, four levels up.
ROOT = os.path.abspath(os.path.join(HERE, "..", "..", "..", ".."))
DEST = os.path.join(ROOT, "target", "vp9vectors")


def fetch(name: str) -> tuple[str, int, str]:
    """Fetch one vector and its MD5 file; the bytes now present, or why not."""
    total = 0
    for suffix in ("", ".md5"):
        target = os.path.join(DEST, name + suffix)
        if os.path.exists(target) and os.path.getsize(target) > 0:
            total += os.path.getsize(target)
            continue
        try:
            with urllib.request.urlopen(BASE + name + suffix, timeout=120) as resp:
                data = resp.read()
        except Exception as e:  # noqa: BLE001 -- reported, not hidden
            return name, total, f"{suffix or 'vector'}: {e}"
        partial = target + ".part"
        with open(partial, "wb") as f:
            f.write(data)
        os.replace(partial, target)
        total += len(data)
    return name, total, ""


def main() -> int:
    with open(os.path.join(HERE, "vectors.txt"), encoding="utf-8") as f:
        names = [line.strip() for line in f if line.strip()]
    os.makedirs(DEST, exist_ok=True)
    # The tests know the suite is here by this file.
    shutil.copyfile(os.path.join(HERE, "vectors.txt"), os.path.join(DEST, "list.txt"))
    failures = 0
    grand = 0
    with concurrent.futures.ThreadPoolExecutor(max_workers=8) as pool:
        for name, size, err in pool.map(fetch, names):
            grand += size
            if err:
                failures += 1
                print(f"FAIL {name}: {err}")
    print(f"{len(names)} vectors in {DEST}, {grand / 1e6:.1f} MB, {failures} failed")
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())

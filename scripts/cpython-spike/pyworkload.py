"""What the proposed Path-Z rung actually reads out of python312.zip.

For requests/b-a-cpython-path-z-self-test.md. The request warns that CPython
"opens a 20 MB zip and reads its central directory"; this replaces that with
the number of bytes that actually cross the ext4 driver.

## Two traps this file exists to avoid

1. **Do not take the module snapshot after importing your own helpers.** A
   first version did `import zipfile` at the top and then reported the startup
   set, counting zipfile's dependency closure -- pathlib, urllib, ipaddress,
   shutil, threading -- as startup work. That inflated the answer ~2x.

2. **Do not decide "came from the zip" by looking at `__file__`.**
   `importlib._bootstrap` and `importlib._bootstrap_external` are *frozen into
   the interpreter* -- they must be, they are what implements importing -- yet
   CPython sets their `__file__` to where the source would have lived, which is
   inside the zip. They are never read from it. Believing `__file__` counted
   117,451 bytes of reads that do not happen, on the two modules guaranteed
   never to be read that way. The loader is the authority: a member is read
   from the archive only if `__spec__.origin` is a real path under the zip AND
   the loader is zipimport's.
"""

import sys

# Line one of user code: everything here was imported by Py_Initialize.
_BEFORE = {m for m in sys.modules}

# Exactly the rung's imports, in the rung's order.
import base64  # noqa: E402
import json  # noqa: E402
import struct  # noqa: E402
import zipimport  # noqa: E402

_AFTER = {m for m in sys.modules}

import os  # noqa: E402
import zipfile  # noqa: E402

# The archive under test. Defaults to whichever zip the running interpreter is
# actually importing from, so the measurement cannot quietly describe a
# different file from the one that produced the module list.
if len(sys.argv) > 1:
    ZIP = sys.argv[1]
else:
    _found = [p for p in sys.path if p.endswith(".zip") and os.path.isfile(p)]
    if not _found:
        sys.exit(
            "no .zip on sys.path and none given on the command line.\n"
            "usage: PYTHONHOME=<prefix> python pyworkload.py [path/to/python312.zip]"
        )
    ZIP = _found[0]

z = zipfile.ZipFile(ZIP)
names = set(z.namelist())


def members(mods: set[str]) -> set[tuple[str, int]]:
    """The archive members `mods` genuinely caused to be read."""
    out = set()
    for name in mods:
        mod = sys.modules.get(name)
        spec = getattr(mod, "__spec__", None)
        if spec is None or spec.origin in (None, "frozen", "built-in"):
            continue
        # zipimport is the only loader that reads out of the archive.
        if type(spec.loader).__name__ != "zipimporter":
            continue
        origin = str(spec.origin)
        if ".zip" not in origin:
            continue
        rel = origin.split(".zip", 1)[1].lstrip("/\\").replace("\\", "/")
        if rel in names:
            out.add((rel, z.getinfo(rel).file_size))
    return out


startup = members(_BEFORE)
workload = members(_AFTER)
added = sorted(workload - startup)

# The End of Central Directory record. zipimport reads the whole central
# directory once, when the archive first appears on sys.path, then seeks to
# individual members. That is the fixed cost of opening the archive -- it is
# emphatically not the archive's 20 MB.
size = os.path.getsize(ZIP)
with open(ZIP, "rb") as f:
    f.seek(size - min(size, 66_000))
    tail = f.read()
idx = tail.rfind(b"PK\x05\x06")
cd_size = struct.unpack("<I", tail[idx + 12 : idx + 16])[0]
cd_count = struct.unpack("<H", tail[idx + 10 : idx + 12])[0]

total = cd_size + sum(n for _, n in workload)
print(f"zip_bytes={size}")
print(f"central_directory_bytes={cd_size}")
print(f"central_directory_entries={cd_count}")
print(f"startup_members={len(startup)} bytes={sum(n for _, n in startup)}")
print(f"workload_added_members={len(added)} bytes={sum(n for _, n in added)}")
print(f"TOTAL_ZIP_BYTES_READ={total}  ({total / size:.2%} of the archive)")
for rel, n in added:
    print(f"  {n:>8}  {rel}")

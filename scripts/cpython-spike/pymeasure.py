"""What a bare `Py_Initialize` reads out of python312.zip -- and the trap.

For requests/b-a-cpython-path-z-self-test.md. Companion to pyworkload.py,
which measures the same thing for the proposed rung's own imports; the two
must agree on the startup figure, and that agreement is the check that the
filter below is right.

Run it under an interpreter that is genuinely importing from the zip -- a
real `<prefix>/bin/python3` + `<prefix>/lib/python312.zip` layout, with
PYTHONHOME set. An interpreter run out of its build directory keeps
`sys.prefix = /usr/local` no matter what PYTHONHOME says, and then measures
a stdlib directory instead of the archive.

    PYTHONHOME=/tmp/zipmeasure /tmp/zipmeasure/bin/python3 pymeasure.py

## Two traps this file exists to demonstrate

1. **Do not take the module snapshot after importing your own helpers.** A
   first version did `import zipfile` at the top and then reported the
   startup set, counting zipfile's dependency closure -- pathlib, urllib,
   ipaddress, shutil, threading -- as startup work. That inflated the answer
   ~2x. `sys` is safe to touch: it is a builtin, imported before any user
   code runs.

2. **Do not decide "came from the zip" by looking at `__file__`.**
   `importlib._bootstrap` and `importlib._bootstrap_external` are *frozen
   into the interpreter* -- they must be, they are what implements importing
   -- yet CPython points their `__file__` at where the source would have
   lived, which is inside the zip. They are never read from it. The loader
   is the authority: a member is read from the archive only if its loader is
   zipimport's. This script prints both numbers so the size of the error is
   visible rather than asserted.
"""

import sys

# Line one of user code: everything here was imported by Py_Initialize.
_STARTUP = [m for m in sys.modules if m not in sys.builtin_module_names]
_STARTUP_MODS = {m: sys.modules[m] for m in _STARTUP}

# Everything below is ours and must not be counted.
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
            "usage: PYTHONHOME=<prefix> python pymeasure.py [path/to/python312.zip]"
        )
    ZIP = _found[0]

z = zipfile.ZipFile(ZIP)
names = set(z.namelist())


def _member(origin: str) -> tuple[str, int] | None:
    """`origin` as an (archive member, uncompressed size) pair, or None."""
    if ".zip" not in origin:
        return None
    rel = origin.split(".zip", 1)[1].lstrip("/\\").replace("\\", "/")
    return (rel, z.getinfo(rel).file_size) if rel in names else None


# The authority: zipimport is the only loader that reads out of the archive.
by_loader: set[tuple[str, int]] = set()
# The trap: what `__file__` claims, which includes the frozen bootstrap pair.
by_file: set[tuple[str, int]] = set()

for name in sorted(_STARTUP):
    mod = _STARTUP_MODS[name]

    hit = _member(str(getattr(mod, "__file__", "") or ""))
    if hit:
        by_file.add(hit)

    spec = getattr(mod, "__spec__", None)
    if spec is None or spec.origin in (None, "frozen", "built-in"):
        continue
    if type(spec.loader).__name__ != "zipimporter":
        continue
    hit = _member(str(spec.origin))
    if hit:
        by_loader.add(hit)

uniq = sorted(by_loader)
overcount = sorted(by_file - by_loader)

print(f"zip_bytes={os.path.getsize(ZIP)}")
print(f"zip_entries={len(names)}")
print(f"startup_modules={len(_STARTUP)}")
print(f"startup_members_from_zip={len(uniq)}")
print(f"startup_bytes_from_zip={sum(n for _, n in uniq)}")
for rel, size in uniq:
    print(f"  {size:>8}  {rel}")

if overcount:
    total = sum(n for _, n in overcount)
    print(f"\n__file__ would also have counted {len(overcount)} member(s), {total} bytes,")
    print("which are frozen into the interpreter and never read from the archive:")
    for rel, size in overcount:
        print(f"  {size:>8}  {rel}")

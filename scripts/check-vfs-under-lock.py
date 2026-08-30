#!/usr/bin/env python3
"""Find module-global lock guards held across a call into the VFS.

The rule
--------
**Do not enter the VFS while holding a module-global lock.**

This is not a style preference; it is the lock order the kernel actually has,
and violating it deadlocks.  `Vfs::readdir` takes the *filesystem's* lock and
calls the filesystem's `readdir` under it.  For procfs that call *generates*
content, and generating `/proc` reaches into arbitrary kernel subsystems --
open-file tables, process lists, network state -- each of which takes its own
module-global lock.  So one live path runs::

    filesystem lock  ->  some module's STATE

Any function that holds a module-global lock and then calls `Vfs::...` runs the
same two locks the other way round::

    some module's STATE  ->  filesystem lock

which is an AB/BA inversion: two CPUs, one in each path, wedge the kernel.

This was not hypothetical.  `fs::handle::{read, write, read_at, write_at,
fstat, ftruncate}` all held `OPEN_FILES` across a VFS call, while procfs's
readdir reached `fs::handle::list_handles`, which takes `OPEN_FILES`.  Lockdep
observed both orders in a single boot (batch 32) once it could name the callers
rather than just `Mutex::<T>::lock`.  Three functions in that same file --
`close`, `read_at_uncached`, `read_dir_at` -- already did it correctly, which
is what a rule with no checker looks like after a while.

The fix is always the same shape: snapshot what the VFS call needs (a path, a
size) under the lock, drop the guard, make the call, then retake the lock and
re-look-up by handle/key to write back the bookkeeping.  Never hold a reference
into the container across the call.

Scope and honesty about it
--------------------------
Like its sibling `check-recursive-locks.py`, this is a *within-one-file*
heuristic that shares that script's parser.  It reports a finding when a named
guard bound from an ALL-CAPS static's `.lock()` is still live at a call whose
callee path mentions the VFS.  It does not resolve imports or trait dispatch,
so it has false negatives by construction (a call into a local helper that
itself calls the VFS is missed unless that helper is in the same file, which
the transitive walk does cover).

Exit codes: 0 clean, 1 findings, 2 could not run.
"""

from __future__ import annotations

import importlib.util
import re
import sys
from pathlib import Path

_SIBLING = Path(__file__).resolve().parent / "check-recursive-locks.py"
_spec = importlib.util.spec_from_file_location("check_recursive_locks", _SIBLING)
if _spec is None or _spec.loader is None:  # pragma: no cover - packaging error
    print(f"error: cannot load {_SIBLING}", file=sys.stderr)
    raise SystemExit(2)
_rl = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(_rl)

# A call into the VFS, however it is spelled at the call site:
#   Vfs::read_at_resolved(..)      crate::fs::Vfs::metadata(..)
#   vfs::Vfs::readdir(..)
VFS_CALL = re.compile(r"\bVfs\s*::\s*([a-z_][a-z0-9_]*)\s*\(")

# Guards that are *not* module-global state and so are not part of this order:
# a lock taken on a local binding cannot be the "A" of an AB/BA with the
# filesystem lock, because nothing else in the kernel can reach it.
#
# `VFS` itself is excluded: `Vfs`'s own methods legitimately hold it, and the
# mount table is inside the filesystem lock order rather than outside it.
EXEMPT_LOCKS = frozenset({"VFS"})

# Files whose whole purpose is to implement the VFS or a filesystem: inside
# them the "filesystem lock" is not an outer lock being inverted, it is the
# subject. Listing them explicitly is better than a path heuristic, because
# the exemption is a claim about each file and should be re-argued if the file
# changes role.
EXEMPT_FILES = frozenset({"fs/vfs.rs", "fs/mount.rs"})


def analyse(path: Path, rel: str) -> list[str]:
    raw = path.read_text(encoding="utf-8", errors="replace")
    src = _rl.strip_noise(raw)
    bodies = _rl.find_bodies(src)
    if not bodies:
        return []
    known = set(bodies)
    memo: dict[str, set[str]] = {}
    findings: list[str] = []

    def reaches_vfs(fn: str, stack: tuple[str, ...] = ()) -> bool:
        """Does `fn` call the VFS, directly or via a same-file callee?"""
        span = bodies.get(fn)
        if span is None or fn in stack:
            return False
        body = src[span[0] : span[1]]
        if VFS_CALL.search(body):
            return True
        return any(
            reaches_vfs(callee, stack + (fn,))
            for callee in _rl.called_names(body, known)
            if callee != fn
        )

    for fn, (bstart, bend) in sorted(bodies.items()):
        # A self-test may hold a lock across VFS calls in ways production code
        # must not; it runs single-threaded at boot with nothing racing it.
        # It is still worth *seeing*, so it is reported, not skipped -- but the
        # marker lets a reader triage at a glance.
        body = src[bstart:bend]
        for bind in _rl.BINDING.finditer(body):
            guard, lock = bind.group(1), bind.group(2)
            if lock in EXEMPT_LOCKS:
                continue
            live_from = bstart + bind.end()
            live_to = _rl.block_end(src, live_from, bend)
            region = src[live_from:live_to]
            d = _rl.DROP.search(region)
            if d and d.group(1) == guard:
                region = region[: d.start()]

            direct = VFS_CALL.search(region)
            via = None
            if direct is None:
                for callee in sorted(_rl.called_names(region, known)):
                    if callee != fn and reaches_vfs(callee):
                        via = callee
                        break
            if direct is None and via is None:
                continue

            line = raw.count("\n", 0, bstart + bind.start()) + 1
            how = (
                f"calls `Vfs::{direct.group(1)}`"
                if direct is not None
                else f"calls `{via}`, which reaches the VFS"
            )
            tag = " [self-test]" if "self_test" in fn or "test" == fn else ""
            findings.append(
                f"{rel}:{line}: `{fn}` holds `{lock}` in `{guard}` and then {how}{tag}"
            )
    return findings


def main() -> int:
    root = Path(__file__).resolve().parent.parent / "kernel" / "src"
    if not root.is_dir():
        print(f"error: no such directory: {root}", file=sys.stderr)
        return 2
    findings: list[str] = []
    files = 0
    for path in sorted(root.rglob("*.rs")):
        rel = path.relative_to(root).as_posix()
        if rel in EXEMPT_FILES:
            continue
        files += 1
        try:
            findings.extend(analyse(path, rel))
        except (OSError, ValueError) as exc:  # keep going; report at the end
            print(f"warning: {path}: {exc}", file=sys.stderr)
    for f in findings:
        print(f)
    print(
        f"\nscanned {files} file(s); "
        f"{len(findings)} guard(s) held across a call into the VFS",
        file=sys.stderr,
    )
    return 1 if findings else 0


if __name__ == "__main__":
    sys.exit(main())

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

import re
import sys
from pathlib import Path

_SIBLING = Path(__file__).resolve().parent / "check-recursive-locks.py"
sys.path.insert(0, str(Path(__file__).resolve().parent))
import selftestflag  # noqa: E402
import srcload  # noqa: E402

# Loaded from source rather than through `importlib`: a `SourceFileLoader`
# consults `__pycache__`, whose staleness check is `(mtime, size)` at
# one-second resolution, so two same-size writes to the sibling inside one
# second leave the second one invisible and this script silently runs the
# previous version of it. See `scripts/srcload.py`.
try:
    _rl = srcload.load(str(_SIBLING), "check_recursive_locks")
except OSError as _exc:  # pragma: no cover - packaging error
    print(f"error: cannot load {_SIBLING}: {_exc}", file=sys.stderr)
    raise SystemExit(2) from _exc

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

# No file is exempt any more. `fs/vfs.rs` used to be -- "inside it the
# filesystem lock is not an outer lock being inverted, it is the subject" --
# and that was true of the mount table and false of the file's other module
# locks. `LOCK_TABLE` (advisory locks) ranks *below* every filesystem lock,
# because procfs's `/proc/locks` takes it with the procfs lock held; and
# `flock`/`funlock`/`lock_query` held it while resolving a path's identity,
# which locks the mounted filesystem. Lockdep caught that AB/BA on the
# 2026-09-26 integration boot, in a file this gate never read. (`fs/mount.rs`
# was exempt too, and no longer exists; an exemption for a missing file only
# waits to hide the next one given that name.)
EXEMPT_FILES: frozenset[str] = frozenset()

# Inside the VFS implementation the VFS is entered differently from outside it.
# Its helpers are called as `Self::name(` -- which the shared call pattern
# deliberately skips, as it skips every `::`-qualified name -- and every path
# into a mounted filesystem goes through `resolve_mount(`, which returns the
# filesystem whose lock the caller then takes. So in this file "a call into
# the VFS" is a call to `resolve_mount`, and `Self::`/`Vfs::` calls are
# followed to their same-file bodies.
VFS_IMPL = "fs/vfs.rs"
RESOLVE_MOUNT = re.compile(r"(?<![\w:.])(resolve_mount)\s*\(")
SELF_CALL = re.compile(r"\b(?:Self|Vfs)\s*::\s*([a-z_][a-z0-9_]*)\s*\(")


def analyse(path: Path, rel: str) -> list[str]:
    return analyse_text(path.read_text(encoding="utf-8", errors="replace"), rel)


def analyse_text(raw: str, rel: str) -> list[str]:
    """Findings for one file's source text; `rel` is its path under kernel/src."""
    in_vfs = rel == VFS_IMPL
    entry = RESOLVE_MOUNT if in_vfs else VFS_CALL
    src = _rl.strip_noise(raw)
    bodies = _rl.find_bodies(src)
    if not bodies:
        return []
    known = set(bodies)
    findings: list[str] = []

    def callees(text: str) -> set[str]:
        names = _rl.called_names(text, known)
        if in_vfs:
            names |= {m.group(1) for m in SELF_CALL.finditer(text) if m.group(1) in known}
        return names

    def reaches_vfs(fn: str, stack: tuple[str, ...] = ()) -> bool:
        """Does `fn` call the VFS, directly or via a same-file callee?"""
        span = bodies.get(fn)
        if span is None or fn in stack:
            return False
        body = src[span[0] : span[1]]
        if entry.search(body):
            return True
        return any(
            reaches_vfs(callee, stack + (fn,))
            for callee in callees(body)
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

            direct = entry.search(region)
            via = None
            if direct is None:
                for callee in sorted(callees(region)):
                    if callee != fn and reaches_vfs(callee):
                        via = callee
                        break
            if direct is None and via is None:
                continue

            line = raw.count("\n", 0, bstart + bind.start()) + 1
            if direct is not None:
                how = (
                    f"calls `{direct.group(1)}`, which locks a mounted filesystem"
                    if in_vfs
                    else f"calls `Vfs::{direct.group(1)}`"
                )
            else:
                how = f"calls `{via}`, which reaches the VFS"
            tag = " [self-test]" if "self_test" in fn or "test" == fn else ""
            findings.append(
                f"{rel}:{line}: `{fn}` holds `{lock}` in `{guard}` and then {how}{tag}"
            )
    return findings


# Fixtures for `--self-test`. Each is (name, path under kernel/src, source,
# number of findings expected). The first is the shape lockdep caught at run
# time on 2026-09-26, in a file this gate did not read: a module lock held in
# vfs.rs while a same-file helper resolves the path's mount.
SELF_TEST_CASES = (
    (
        "vfs.rs: a side table held across Self:: into resolve_mount is reported",
        VFS_IMPL,
        """
static LOCK_TABLE: Mutex<Vec<u8>> = Mutex::new(Vec::new());
impl Vfs {
    pub fn flock_resolved(path: &Path) -> KernelResult<()> {
        let mut table = LOCK_TABLE.lock();
        let id = Self::file_identity_resolved(path).unwrap_or(None);
        table.push(0);
        Ok(())
    }
    pub fn file_identity_resolved(path: &Path) -> KernelResult<Option<u64>> {
        let (fs, _id, _opts, rel) = resolve_mount(path)?;
        fs.lock().identity(&rel)
    }
}
""",
        1,
    ),
    (
        "vfs.rs: the same table taken after the identity is resolved is clean",
        VFS_IMPL,
        """
static LOCK_TABLE: Mutex<Vec<u8>> = Mutex::new(Vec::new());
impl Vfs {
    pub fn flock_resolved(path: &Path) -> KernelResult<()> {
        let id = Self::file_identity_resolved(path).unwrap_or(None);
        let mut table = LOCK_TABLE.lock();
        table.push(0);
        Ok(())
    }
    pub fn file_identity_resolved(path: &Path) -> KernelResult<Option<u64>> {
        let (fs, _id, _opts, rel) = resolve_mount(path)?;
        fs.lock().identity(&rel)
    }
}
""",
        0,
    ),
    (
        "vfs.rs: the mount table itself stays exempt",
        VFS_IMPL,
        """
static VFS: Mutex<VfsInner> = Mutex::new(VfsInner::new());
impl Vfs {
    pub fn remount(path: &Path) -> KernelResult<()> {
        let inner = VFS.lock();
        let found = resolve_mount(path)?;
        Ok(())
    }
}
""",
        0,
    ),
    (
        "outside vfs.rs: a module lock held across Vfs:: is still reported",
        "fs/handle.rs",
        """
static OPEN_FILES: Mutex<Vec<u8>> = Mutex::new(Vec::new());
pub fn read(h: u64) -> KernelResult<()> {
    let files = OPEN_FILES.lock();
    Vfs::read_at_resolved(h)?;
    Ok(())
}
""",
        1,
    ),
    (
        "outside vfs.rs: resolve_mount is not the entry, so a local of that name is not flagged",
        "net/route.rs",
        """
static ROUTES: Mutex<Vec<u8>> = Mutex::new(Vec::new());
fn resolve_mount(x: u64) -> u64 { x }
pub fn pick(x: u64) -> u64 {
    let r = ROUTES.lock();
    resolve_mount(x)
}
""",
        0,
    ),
)


def self_test() -> int:
    failed = 0
    for name, rel, source, want in SELF_TEST_CASES:
        got = analyse_text(source, rel)
        ok = len(got) == want
        print(f"  {'ok  ' if ok else 'FAIL'}  {name} ({len(got)} finding(s), want {want})")
        if not ok:
            failed += 1
            for line in got:
                print(f"          {line}")
    print(f"check-vfs-under-lock: self-test {'passed' if not failed else 'FAILED'} "
          f"({failed} failure(s))")
    return 1 if failed else 0


def main() -> int:
    args = sys.argv[1:]
    # Every spelling (`--self-test`, `--selftest`): a mistyped one must not
    # fall through to the real scan and exit 0 having tested nothing.
    if len(args) == 1 and selftestflag.wants_selftest(args):
        return self_test()
    root = Path(__file__).resolve().parent.parent / "kernel" / "src"
    if len(args) == 2 and args[0] == "--root":
        # Another tree's kernel sources: for checking a change to this script
        # from outside the tree it grades.
        root = Path(args[1])
    elif args:
        print("usage: check-vfs-under-lock.py [--self-test | --root <kernel/src>]",
              file=sys.stderr)
        return 2
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

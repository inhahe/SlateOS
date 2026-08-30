#!/usr/bin/env python3
"""Guard the single VFS permission gate against being bypassed or duplicated.

The rule
--------
**Exactly one place in the kernel decides whether a path may be accessed, and
every path-taking VFS entry point asks it.**

That place is `fs::vfs::check_path_access(path, PathAccess)`, whose decision
half is `fs::vfs::path_access_verdict`.  It consults, in order,
`cap::file_tags::check_access` (mandatory-access-control tags) and
`fs::acl::check_access` (POSIX 1003.1e ACLs).

Why a checker and not just a code review
----------------------------------------
Because the previous arrangement failed in both directions at once, silently:

* `cap::file_tags::check_access` was called from **sixteen** hand-written sites
  in `fs/vfs.rs` plus a seventeenth hand-copied into `fs/handle.rs`.  A hook you
  must remember to write at every entry point is a hook the *next* entry point
  will not have -- and several already did not.
* `fs::acl::check_access` was called from **zero**.  It implements the entire
  POSIX ACL evaluation algorithm; `setfacl` validated and stored ACLs, `getfacl`
  read them back, procfs counted them, and no file operation ever consulted one.
  A security feature that reports success while governing nothing is worse than
  an absent one, because an absent feature is visible.

Neither failure is detectable by a test that exercises *allowed* access, since
both checks fail open when they find nothing to say.  Only a source-level
invariant catches "somebody added a new entry point and forgot".

What is checked
---------------
1. `acl::check_access` is called from exactly one place: the gate.
2. `file_tags::check_access` is called from exactly one place: the gate.
3. Every `pub`/`pub(crate)` method of `impl Vfs` that resolves a path to a
   filesystem itself (calls `resolve_mount`) either calls `check_path_access`
   or appears in `UNGATED` below with a stated reason.
4. `Vfs::metadata_resolved` is *not* gated.  This one is a recursion guard: the
   ACL half of the gate needs the file's owning uid/gid, which it obtains via
   `metadata_resolved`; gating that would re-enter the gate forever.

Scope and honesty about it
--------------------------
This is a textual, single-file heuristic in the style of its siblings
`check-vfs-under-lock.py` and `check-recursive-locks.py`, and shares their
parser.  It does not resolve imports or trait dispatch.  Rule 3 in particular
keys off `resolve_mount` as the marker for "this method touches a real path on
a real filesystem"; a future entry point that reaches a filesystem some other
way would be missed.  That is a false negative by construction and the reason
`UNGATED` demands a reason rather than just a name -- the list is the audit.

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

GATE_FILE = "fs/vfs.rs"

# The one function allowed to call each underlying check.
ACL_GATE = "check_acl"
TAGS_GATE = "path_access_verdict"

ACL_CALL = re.compile(r"\bacl\s*::\s*check_access\s*\(")
TAGS_CALL = re.compile(r"\bfile_tags\s*::\s*check_access\s*\(")
GATE_CALL = re.compile(r"\bcheck_path_access\s*\(")
RESOLVE_MOUNT = re.compile(r"\bresolve_mount\s*\(")

# `pub fn` / `pub(crate) fn` / `pub(super) fn`, capturing the name.
PUB_FN = re.compile(r"\bpub(?:\s*\([^)]*\))?\s+fn\s+([a-z_][a-z0-9_]*)\s*[(<]")

# Methods that reach a filesystem but must NOT carry the gate.  Every entry
# needs a reason, and the reason is the point: this list is where a reader
# checks whether an exemption is still true.
UNGATED: dict[str, str] = {
    # --- recursion: the gate itself calls these ---
    "metadata_resolved": (
        "the ACL half of the gate needs the file's uid/gid and gets them here; "
        "gating this re-enters the gate forever"
    ),
    # --- the gate's own decision path ---
    "access": (
        "this IS a permission query; it calls check_path_access once per "
        "requested class (R_OK/W_OK/X_OK) rather than once for the call"
    ),
    # --- mount-table administration, not file access ---
    "mount": "mounts a filesystem; the path names a mount point, not a file",
    "unmount": "unmounts a filesystem; guarded by capability, not by path ACL",
    "remount": "changes mount options; guarded by capability, not by path ACL",
    "mount_options": "reads mount options, not file contents or metadata",
    "is_mount_point": "answers a question about the mount table",
    "statfs": "reports filesystem-wide capacity, not anything about a file",
    "statvfs": (
        "reports filesystem-wide capacity; the path only selects which mount, "
        "and nothing about the path itself is read or written"
    ),
    "trim": (
        "discards the filesystem's free blocks (the kernel side of fstrim(8)); "
        "the path only selects which mount, and free space belongs to no file"
    ),
    "set_volume_label": (
        "renames the filesystem, not a file on it; a whole-device operation "
        "that ACLs on individual paths have no opinion about"
    ),
    "sync_path": "flushes a filesystem's own cache; takes no per-file decision",
    # --- callers that delegate to a gated sibling ---
    # (none: delegating methods do not call resolve_mount, so they never reach
    # rule 3.  Kept as a heading so the next person knows the category exists.)
}


def _bodies_with_names(src: str) -> dict[str, tuple[int, int]]:
    return _rl.find_bodies(src)


def _impl_vfs_span(src: str) -> tuple[int, int] | None:
    """Byte span of `impl Vfs { ... }` in noise-stripped source."""
    m = re.search(r"\bimpl\s+Vfs\s*\{", src)
    if m is None:
        return None
    depth = 0
    i = m.end() - 1
    while i < len(src):
        c = src[i]
        if c == "{":
            depth += 1
        elif c == "}":
            depth -= 1
            if depth == 0:
                return (m.end(), i)
        i += 1
    return None


def check_single_callers(root: Path) -> list[str]:
    """Rules 1 and 2: each underlying check has exactly one caller."""
    findings: list[str] = []
    for label, pattern, owner in (
        ("acl::check_access", ACL_CALL, ACL_GATE),
        ("file_tags::check_access", TAGS_CALL, TAGS_GATE),
    ):
        sites: list[tuple[str, int, str]] = []
        for path in sorted(root.rglob("*.rs")):
            rel = path.relative_to(root).as_posix()
            raw = path.read_text(encoding="utf-8", errors="replace")
            src = _rl.strip_noise(raw)
            bodies = _bodies_with_names(src)
            for m in pattern.finditer(src):
                # The definition site itself is `pub fn check_access(` -- the
                # pattern requires a `acl::`/`file_tags::` qualifier, so it
                # cannot match a definition.
                line = raw.count("\n", 0, m.start()) + 1
                fn = "<top level>"
                for name, (b, e) in bodies.items():
                    if b <= m.start() < e:
                        fn = name
                        break
                sites.append((rel, line, fn))
        wrong = [s for s in sites if not (s[0] == GATE_FILE and s[2] == owner)]
        if not sites:
            findings.append(
                f"{GATE_FILE}: `{label}` has no callers at all -- the gate is "
                f"not wired in, and the check governs nothing"
            )
        for rel, line, fn in wrong:
            findings.append(
                f"{rel}:{line}: `{label}` called from `{fn}`; the only caller "
                f"may be `{GATE_FILE}::{owner}` (route it through "
                f"`check_path_access` instead)"
            )
    return findings


def check_entry_points(root: Path) -> list[str]:
    """Rules 3 and 4: path-taking Vfs methods carry the gate."""
    path = root / "fs" / "vfs.rs"
    if not path.is_file():
        return [f"error: no such file: {path}"]
    raw = path.read_text(encoding="utf-8", errors="replace")
    src = _rl.strip_noise(raw)
    span = _impl_vfs_span(src)
    if span is None:
        return [f"{GATE_FILE}: cannot find `impl Vfs {{`; the checker is stale"]
    istart, iend = span
    bodies = _bodies_with_names(src)

    findings: list[str] = []
    seen: set[str] = set()
    for m in PUB_FN.finditer(src, istart, iend):
        name = m.group(1)
        body_span = bodies.get(name)
        if body_span is None or name in seen:
            continue
        seen.add(name)
        body = src[body_span[0] : body_span[1]]
        gated = GATE_CALL.search(body) is not None

        if name == "metadata_resolved":
            if gated:
                findings.append(
                    f"{GATE_FILE}: `Vfs::metadata_resolved` calls "
                    f"`check_path_access`; the gate reads file ownership "
                    f"through it, so this recurses without bound"
                )
            continue

        if RESOLVE_MOUNT.search(body) is None:
            continue
        if gated:
            if name in UNGATED:
                findings.append(
                    f"{GATE_FILE}: `Vfs::{name}` is listed in UNGATED but does "
                    f"call `check_path_access`; drop the stale exemption"
                )
            continue
        if name in UNGATED:
            continue
        line = raw.count("\n", 0, body_span[0]) + 1
        findings.append(
            f"{GATE_FILE}:{line}: `Vfs::{name}` resolves a path to a "
            f"filesystem without calling `check_path_access` -- add the gate, "
            f"or add it to UNGATED with a reason"
        )
    return findings


def main() -> int:
    root = Path(__file__).resolve().parent.parent / "kernel" / "src"
    if not root.is_dir():
        print(f"error: no such directory: {root}", file=sys.stderr)
        return 2
    findings = check_single_callers(root) + check_entry_points(root)
    for f in findings:
        print(f)
    print(
        f"\n{len(findings)} permission-gate finding(s)",
        file=sys.stderr,
    )
    return 1 if findings else 0


if __name__ == "__main__":
    sys.exit(main())

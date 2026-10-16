#!/usr/bin/env python3
"""Keep kernel writes to user memory confined to the validated primitives.

Why this exists
---------------
The kernel reaches user memory two ways, and the hardware only polices one of
them.

**Through the user virtual address.** SMAP makes this self-enforcing: a ring-0
access to a user page faults unless it sits inside a ``stac()`` / ``clac()``
window, and SMAP is enabled and verified at every boot.  So a path that forgets
the window is reported by the CPU on its first execution -- loudly, immediately,
and without help from this script.  What SMAP does *not* check is whether the
range was validated for the right thing: a site that opens a window and writes
after only a *read* validation is invisible to it.  Confining ``stac()`` to
``mm/user.rs`` is what makes that answerable by reading one file.

**Through the HHDM alias of the page's physical frame.** This is the dangerous
one, and it is dangerous precisely because nothing stops it.  The HHDM address
is a *kernel* address in a legitimately writable mapping, so:

  * SMAP does not apply -- it guards user addresses, and this is not one;
  * the page's own write-protect bit does not apply -- it is a property of the
    user mapping being bypassed, not of the alias being used.

A write through the alias to a page that is present-but-read-only therefore
succeeds where the corresponding user-address write would have faulted.  When
that page is copy-on-write -- which, after a ``fork``, is *every* page of the
address space -- the write lands in the frame the parent is still using.  Two
processes silently diverge from one page of memory, with no fault, no log line
and no failing test.  That is strictly worse than the ring-0 #PF tracked as
``W-KERNEL-COW-WRITE``, which at least announces itself.

``mm::user::copy_to_user_as`` is the primitive that does this correctly: it
resolves via ``user_page_phys``, which checks ``WRITABLE`` through
``translate_flags`` and asks the owning process's fault resolver to break a CoW
or populate a committed-but-absent page before the walk is retried.  Plain
``page_table::translate`` does none of that -- it reports the frame behind a
mapping and says nothing about its flags.

This script exists because ``proc/linux_stack.rs`` had grown its own copy of
that loop (``write_user_image``) built on plain ``translate``, and it had
silently lost the ``WRITABLE`` check, the CoW break and the bounds check.  It
was unreachable in practice -- its one caller mapped the destination eagerly --
but it held by construction *elsewhere*, which is not a property the code
itself had.  See design-decisions.md, and ``W-KERNEL-COW-WRITE`` in
known-issues.md for the audit this came out of.

Scope and honesty about it
--------------------------
Check 1 (``stac()`` confinement) is exact: it is a search for one token.

Check 2 (HHDM write) is a *heuristic*.  It looks for a ``page_table::translate``
whose result is combined with an HHDM base and then used as a ``*mut``, within a
short window of lines, in a file that is not an approved home for the pattern.
It therefore:

  * has false negatives -- an alias laundered through a helper, or through a
    struct field, is not seen;
  * may have false positives -- a legitimate write to a frame the kernel owns
    outright (a freshly allocated frame not yet mapped to anyone) looks the
    same from here.

Every report is meant to be read before anything is changed.  The fix is
usually "call ``mm::user::copy_to_user_as``"; where it genuinely is not, add the
file to ``ALLOWED_HHDM_WRITERS`` with a comment saying why, which is a decision
the next reader can then see and re-examine.

Self-test
---------
``--self-test`` runs both checks over a fixture tree built in a temporary
directory -- a SMAP window in an allowed file and in a forbidden one, in code
and in a comment; an HHDM write through a translated frame in an allowed file,
in a forbidden one, bound by ``let`` and by a match arm, inside a
``self_test_*`` body, after a presence-only probe, and with the write before
the alias -- and checks each verdict. The push hook runs it before trusting a
real verdict (design-decisions §974).

Exit codes: 0 clean, 1 findings, 2 could not run.
"""

from __future__ import annotations

import re
import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import selftestflag  # noqa: E402  (the scripts directory, put on the path above)

REPO_ROOT = Path(__file__).resolve().parent.parent
KERNEL_SRC = REPO_ROOT / "kernel" / "src"

# --- Check 1: who may open a SMAP window ------------------------------------
#
# `mm/user.rs` holds the validated primitives every other caller is expected to
# go through. `smep_smap.rs` implements stac/clac itself and self-tests that
# enforcement is really on, so it necessarily names them.
ALLOWED_STAC_FILES = {
    "mm/user.rs",
    "smep_smap.rs",
}

STAC_RE = re.compile(r"\bstac\s*\(\s*\)")
# The definition and its doc comments in `smep_smap.rs` are not call sites, but
# that file is allowed wholesale anyway, so no exemption is needed here.

# --- Check 2: who may write through an HHDM alias of a translated user page --
#
# These are the files where resolving a user VA to its frame and writing through
# the physical alias is the *point*, not an accident:
ALLOWED_HHDM_WRITERS = {
    # The primitives themselves. `copy_to_user_as` and friends are the correct
    # implementation of this pattern; everything else should call them.
    "mm/user.rs",
    # Frame lifecycle: zeroing/copying frames the kernel owns outright, before
    # or after they belong to any address space. CoW breaking lives here too --
    # copying the old frame to the new one is the operation, not a bypass of it.
    "mm/cow.rs",
    "mm/frame.rs",
    "mm/page_table.rs",
    # Swap reads a page out through the alias before unmapping it. A read of a
    # CoW page is correct whichever sharer's view you take, so the hazard this
    # script is about does not arise.
    "mm/swap.rs",
    # The ELF loader populates freshly allocated frames for an address space
    # that no process has entered yet.
    "proc/elf.rs",
}

TRANSLATE_RE = re.compile(r"page_table::translate\s*\(")
HHDM_RE = re.compile(r"\bhhdm\b")
WRITE_RE = re.compile(r"as \*mut |\*mut u8|from_raw_parts_mut|write_volatile|write_bytes")

# What the translated frame address got bound to, if anything.  Three shapes
# cover every site in the tree:
#
#     let phys = page_table::translate(..)          -> `phys`
#     let Some(phys) = page_table::translate(..)    -> `phys`
#     match page_table::translate(..) { Some(phys)  -> `phys` (next lines)
#
# The binding is the whole point of the check.  The hazard is writing through
# *the frame `translate` just found*, i.e. through the alias of a page that is
# mapped into somebody's address space.  A `translate` whose result is only
# tested for presence and then dropped -- `if translate(..).is_some() { continue }`
# guarding a write to a frame the caller allocated itself -- is the opposite
# situation: it proves the page is absent before writing.  Requiring the write
# to name the binding tells those two apart, which a bare "did `translate` and
# `*mut` both appear nearby" test cannot.
BIND_RE = re.compile(r"(?:let\s+(?:Some\s*\(\s*)?|Some\s*\(\s*)(\w+)\s*\)?\s*=\s*page_table::translate")
MATCH_ARM_BIND_RE = re.compile(r"Some\s*\(\s*(\w+)\s*\)\s*=>")

# How far after a `translate` to keep looking for the alias-write pair. The
# linux_stack.rs case spanned 16 lines from `translate` to `copy_nonoverlapping`;
# 30 leaves room without sweeping in unrelated code below.
WINDOW = 30

# A `self_test_*` function builds the address space it writes into -- it
# allocates the frames, stamps them, and tears them down -- so a write through a
# frame it just mapped itself is the test doing its job, not a path that could
# meet a caller's CoW page.  Production code gets no such exemption, which is
# what keeps the 50k-line `syscall/linux.rs` covered rather than whitelisted
# wholesale for the sake of the self-tests at the bottom of it.
FN_RE = re.compile(r"^\s*(?:pub(?:\([^)]*\))?\s+)?(?:const\s+)?(?:unsafe\s+)?fn\s+(\w+)")


def rel(path: Path, root: Path = KERNEL_SRC) -> str:
    """Path relative to `root` (`kernel/src`), with forward slashes on every
    platform."""
    return path.relative_to(root).as_posix()


def check_stac_confinement(files: list[Path], root: Path = KERNEL_SRC) -> list[str]:
    """Report SMAP windows opened outside the validated primitives."""
    findings = []
    for path in files:
        name = rel(path, root)
        if name in ALLOWED_STAC_FILES:
            continue
        try:
            lines = path.read_text(encoding="utf-8", errors="replace").splitlines()
        except OSError as exc:
            findings.append(f"{name}: unreadable ({exc})")
            continue
        for n, line in enumerate(lines, 1):
            code = line.split("//", 1)[0]
            if STAC_RE.search(code):
                findings.append(
                    f"{name}:{n}: opens a SMAP window outside mm/user.rs\n"
                    f"    {line.strip()}\n"
                    "    A kernel access to user memory belongs behind one of the\n"
                    "    validated primitives in mm::user, which pair the window with\n"
                    "    a validation of the matching direction. SMAP cannot tell a\n"
                    "    write validated for reading from one validated for writing."
                )
    return findings


def check_hhdm_writes(files: list[Path], root: Path = KERNEL_SRC) -> list[str]:
    """Report probable writes through the HHDM alias of a translated user page."""
    findings = []
    for path in files:
        name = rel(path, root)
        if name in ALLOWED_HHDM_WRITERS:
            continue
        try:
            lines = path.read_text(encoding="utf-8", errors="replace").splitlines()
        except OSError as exc:
            findings.append(f"{name}: unreadable ({exc})")
            continue

        code = [ln.split("//", 1)[0] for ln in lines]

        # Enclosing function per line, so `self_test_*` bodies can be skipped.
        enclosing = []
        current = ""
        for ln in code:
            m = FN_RE.match(ln)
            if m:
                current = m.group(1)
            enclosing.append(current)

        for n, line in enumerate(code):
            if not TRANSLATE_RE.search(line):
                continue
            if enclosing[n].startswith("self_test"):
                continue

            # What did the frame address get bound to?  No binding means the
            # result was only probed for presence, which is not this hazard.
            m = BIND_RE.search(line)
            if m:
                binding = m.group(1)
            else:
                arm = next(
                    (
                        a.group(1)
                        for a in (MATCH_ARM_BIND_RE.search(w) for w in code[n : n + 4])
                        if a
                    ),
                    None,
                )
                if arm is None:
                    continue
                binding = arm

            bind_use = re.compile(rf"\b{re.escape(binding)}\b")
            window = code[n : n + WINDOW]
            # The alias and the write must both appear, the alias must come
            # first, and the write must actually go through the translated
            # frame -- otherwise this is some unrelated `*mut` further down.
            hhdm_at = next(
                (i for i, w in enumerate(window) if HHDM_RE.search(w) and bind_use.search(w)),
                None,
            )
            if hhdm_at is None:
                continue
            write_at = next(
                (i for i, w in enumerate(window) if i >= hhdm_at and WRITE_RE.search(w)),
                None,
            )
            if write_at is None:
                continue
            findings.append(
                f"{name}:{n + 1}: resolves a page with page_table::translate and\n"
                f"    appears to write through its HHDM alias "
                f"(alias line {n + 1 + hhdm_at}, write line {n + 1 + write_at}):\n"
                f"        {lines[n].strip()}\n"
                f"        {lines[n + write_at].strip()}\n"
                "    `translate` reports the frame without regard to the mapping's\n"
                "    WRITABLE flag, and a write through the alias is a kernel-address\n"
                "    write that neither SMAP nor the write-protect bit can stop. On a\n"
                "    copy-on-write page -- every page of a freshly forked address\n"
                "    space -- that corrupts the other sharer silently.\n"
                "    Use mm::user::copy_to_user_as, which checks WRITABLE and breaks\n"
                "    the CoW first. If this frame is not in any address space yet,\n"
                "    add the file to ALLOWED_HHDM_WRITERS with a note saying so."
            )
    return findings


# --- Self-test ---------------------------------------------------------------
#
# Each case is a file under a fixture `kernel/src`, and whether its name must
# appear in the findings. Written as the tree writes these things, so a change
# to a pattern that stops matching the real shape fails here first.

_TRANSLATE_WRITE = """
fn poke(va: u64, hhdm: u64) {
    let phys = page_table::translate(va);
    let alias = (phys + hhdm) as *mut u8;
    unsafe { alias.write_volatile(0) };
}
"""

_SELFTEST_CASES = [
    # (relative path, contents, must be reported)
    ("fs/opens_window.rs", "fn f() { stac(); copy(); clac(); }\n", True),
    ("mm/user.rs", "fn f() { stac(); copy(); clac(); }\n", False),
    ("fs/comment_only.rs", "fn f() {} // callers must not stac() here\n", False),
    ("proc/writes_alias.rs", _TRANSLATE_WRITE, True),
    ("mm/cow.rs", _TRANSLATE_WRITE, False),
    (
        "proc/match_arm.rs",
        """
fn poke(va: u64, hhdm: u64) {
    match page_table::translate(va) {
        Some(frame) => {
            let p = (frame + hhdm) as *mut u8;
            unsafe { p.write_volatile(1) };
        }
        None => {}
    }
}
""",
        True,
    ),
    (
        "proc/in_self_test.rs",
        _TRANSLATE_WRITE.replace("fn poke", "fn self_test_poke"),
        False,
    ),
    (
        "proc/presence_probe.rs",
        """
fn fill(va: u64, frame: u64, hhdm: u64) {
    if page_table::translate(va).is_some() {
        return;
    }
    let p = (frame + hhdm) as *mut u8;
    unsafe { p.write_volatile(2) };
}
""",
        False,
    ),
    (
        # An alias of a frame the caller holds, not of the one translated.
        "proc/other_frame.rs",
        """
fn h(va: u64, frame: u64, hhdm: u64) {
    let phys = page_table::translate(va);
    let p = (frame + hhdm) as *mut u8;
    unsafe { p.write_volatile(4) };
    drop(phys);
}
""",
        False,
    ),
    (
        # A write in the window but before the alias is some other pointer.
        "proc/write_first.rs",
        """
fn g(dst: *mut u8, va: u64, hhdm: u64) -> u64 {
    let phys = page_table::translate(va);
    unsafe { dst.write_volatile(3) };
    phys + hhdm
}
""",
        False,
    ),
]


def self_test() -> int:
    """Run both checks over the fixture cases; 0 if every verdict is right."""
    failures = 0
    with tempfile.TemporaryDirectory(prefix="uaccess-selftest-") as tmp:
        root = Path(tmp) / "kernel" / "src"
        for name, body, _ in _SELFTEST_CASES:
            path = root / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(body, encoding="utf-8", newline="\n")
        files = sorted(root.rglob("*.rs"))
        findings = check_stac_confinement(files, root) + check_hhdm_writes(files, root)
        for name, _, reported in _SELFTEST_CASES:
            got = any(f.startswith(f"{name}:") for f in findings)
            verdict = "reported" if got else "passed"
            want = "reported" if reported else "passed"
            if got != reported:
                failures += 1
                print(f"  FAIL  {name}: {verdict}, should have been {want}")
            else:
                print(f"  ok    {name}: {verdict}")
        # Nothing but the fixture cases may be reported: a finding naming a
        # file no case expects means a check matched something it should not.
        stray = [f for f in findings if not any(f.startswith(f"{n}:") for n, _, r in _SELFTEST_CASES if r)]
        for f in stray:
            failures += 1
            print(f"  FAIL  unexpected finding: {f.splitlines()[0]}")
    total = len(_SELFTEST_CASES)
    if failures:
        print(f"[user-access] self-test: {failures} of {total} case(s) FAILED", file=sys.stderr)
        return 1
    print(f"[user-access] self-test passed ({total} cases)")
    return 0


def main() -> int:
    unknown = selftestflag.unknown_options(sys.argv[1:])
    if unknown:
        print(f"check-user-access-sites: unrecognised option {unknown[0]!r}", file=sys.stderr)
        return 2
    if selftestflag.wants_selftest(sys.argv[1:]):
        return self_test()
    if not KERNEL_SRC.is_dir():
        print(f"check-user-access-sites: no {KERNEL_SRC}", file=sys.stderr)
        return 2

    files = sorted(KERNEL_SRC.rglob("*.rs"))
    if not files:
        print(f"check-user-access-sites: no .rs files under {KERNEL_SRC}", file=sys.stderr)
        return 2

    findings = check_stac_confinement(files) + check_hhdm_writes(files)

    if not findings:
        print(
            f"[user-access] {len(files)} file(s): SMAP windows confined to "
            "mm/user.rs, no unreviewed HHDM writes to translated pages"
        )
        return 0

    for f in findings:
        print(f"[user-access] {f}")
    print(f"\n[user-access] {len(findings)} finding(s)")
    return 1


if __name__ == "__main__":
    sys.exit(main())

#!/usr/bin/env python3
"""Fail if a kernel self-test clears another module's state without moving it aside.

The hazard, in one sentence
--------------------------

A suite that wants an empty table often gets one by calling the owning module's
`clear_all()`. On a live machine that deregisters every service, or discards the
operator's whole event history, and then the suite reports success.

`crate::fs::selftest::with_pristine` does not help: its guarantee is that *this*
module's state comes back, and it says nothing about anything the suite reaches
into. The remedy the tree uses is a `pub(crate) fn with_pristine_state` on the
reached-into module, composed by the reaching suite -- see
`svcstart::self_test` and `logpersist::self_test`.

Why this exists as a tracked gate
---------------------------------

`known-issues.md` → `TD-A-SELFTESTS-REACH-OUTSIDE-THEIR-OWN-MODULE` records three
instances, all fixed on 2026-08-23, and names the check that found them:
`build/survey_reach.py`. **`build/` is gitignored** (`.gitignore:88`), so that
script was never tracked, is absent from the tree, and nothing has re-run the
survey since. A fourth instance arrived in the meantime: `sockact::self_test`
called `servicemgr::clear_all()` at both ends of its body, leaving the registry
empty, and it ran at every boot. Found 2026-09-11 by re-deriving the survey.

A checker that lives only in a gitignored directory is a checker that one clean
checkout deletes. That is the whole reason this file is here rather than there.

What it asks, and what it cannot ask
------------------------------------

For every function whose name begins `self_test`, it looks for calls of the form
`<module>::<verb>(` where `<verb>` names a state-clearing operation and
`<module>` is not the file's own. A hit is a finding unless either:

* the file also composes `<module>::with_pristine_state` -- the tree's remedy; or
* the pair is in `ALLOWED` below with a reason.

**The verb list is a filter, not a proof**, and that is stated rather than hidden:
a destructive function with an unexpected name is invisible here, exactly as the
original survey's "a call that leaves the module through a helper *in* the module
is invisible to it". Two things make the filter worth having anyway. It caught a
real instance the day it was written. And the allowlist forces a sentence about
each surviving pair, which is what turns "nothing obvious" into "these five,
because".

The wrapper check is **per file**, not per suite. A file that composes
`X::with_pristine_state` for one suite and reaches into `X` from another is not
flagged. Tightening that needs the reach's enclosing public entry point, which
`rust_scopes` can give -- worth doing if this gate ever has to grade a file with
two suites in it. Today no file in the tree does.

Exit codes follow `run_checker`: `0` clean, `1` finding, and `--self-test`
returns `0`/`1` for its own cases.
"""

from __future__ import annotations

import argparse
import pathlib
import re
import sys

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))

import rust_scopes  # noqa: E402
import rustlex  # noqa: E402

# Operations whose effect is "the state that was there is gone". Derived from the
# names that actually appear in this tree rather than invented: `clear_all` and
# `clear` are what the three documented instances called, `init_defaults` is what
# they called next, and the rest are the same idea under other spellings.
# Spelled out rather than written literally, so that a patch applied with a
# heredoc cannot collapse it -- a mistake this session made six times.
NL_ = chr(10)

VERBS = (
    "clear_all",
    "clear",
    "reset",
    "reset_all",
    "remove_all",
    "delete_all",
    "purge",
    "wipe",
    "drop_all",
    "truncate_all",
    "init_defaults",
    "clear_history",
    "deinit",
)

REACH = re.compile(
    r"\b(?:crate::)?(?:[a-z_][a-z0-9_]*::)*?([a-z_][a-z0-9_]*)::(" + "|".join(VERBS) + r")\s*\("
)

# (file module, reached module, verb) -> why it is safe unwrapped.
ALLOWED: dict[tuple[str, str, str], str] = {
    ("linux", "nameservice", "init_defaults"): (
        "Idempotent and non-destructive: nameservice::init_defaults opens with "
        "`if guard.is_some() { return; }`, so on a machine whose name service is "
        "already configured it does nothing at all. It seeds only a None state, "
        "which is what a fresh boot holds anyway."
    ),
    ("linux", "restart_block", "clear"): (
        "Scoped to task ids the suite itself created, not a table-wide wipe -- "
        "`restart_block::clear(t1)` names one task. Nothing belonging to a task "
        "the suite did not make is reachable through it."
    ),
}


def own_module(rel_path: str) -> str:
    """The module name a file defines, as a reach would name it."""
    parts = rel_path.replace("\\", "/").removesuffix(".rs").split("/")
    if parts[-1] == "mod" and len(parts) > 1:
        return parts[-2]
    return parts[-1]


def findings_for(text: str, rel_path: str) -> list[tuple[int, str, str]]:
    """Every UNWRAPPED destructive reach in `text`'s self-tests.

    Returns `(line_number, reached_module, verb)`. Pure over its inputs so that
    `--self-test` can exercise it on synthetic sources -- the scope walk and the
    masking are the parts that have been wrong before, and they are both in here.

    Wrapped reaches are excluded, so this is deliberately NOT the number to
    report as a total: see `wrapped_count`, which exists because the first
    version of the summary said "0 wrapped" on a tree with four of them.
    """
    mine = own_module(rel_path)
    masked = rustlex.strip_noise(text, keep_literals=False).splitlines()
    stacks = rust_scopes.scope_stack_per_line(text.splitlines())
    # The remedy, wherever it appears in the file. See the module docstring on why
    # this is per-file rather than per-suite.
    wrapped = {
        m.group(1)
        for m in re.finditer(r"\b([a-z_][a-z0-9_]*)::with_pristine_state\s*\(", "\n".join(masked))
    }
    out: list[tuple[int, str, str]] = []
    for i, line in enumerate(masked):
        stack = stacks[i] if i < len(stacks) else []
        if not any(s.name and s.name.startswith("self_test") for s in stack):
            continue
        for m in REACH.finditer(line):
            reached, verb = m.group(1), m.group(2)
            if reached in (mine, "self", "Self"):
                continue
            if reached in wrapped:
                continue
            out.append((i + 1, reached, verb))
    return out


def wrapped_count(text: str, rel_path: str) -> int:
    """Destructive reaches that ARE covered by the reached module's wrapper.

    The same walk as `findings_for` with the `wrapped` test inverted. Counted
    separately rather than returned alongside, because the two numbers answer
    different questions -- one is a backlog, the other is evidence the remedy is
    in use -- and the first version of this gate conflated them into a figure that
    was always zero.
    """
    mine = own_module(rel_path)
    masked = rustlex.strip_noise(text, keep_literals=False).splitlines()
    stacks = rust_scopes.scope_stack_per_line(text.splitlines())
    wrapped = {
        m.group(1)
        for m in re.finditer(r"\b([a-z_][a-z0-9_]*)::with_pristine_state\s*\(", "\n".join(masked))
    }
    n = 0
    for i, line in enumerate(masked):
        stack = stacks[i] if i < len(stacks) else []
        if not any(s.name and s.name.startswith("self_test") for s in stack):
            continue
        for m in REACH.finditer(line):
            reached = m.group(1)
            if reached in (mine, "self", "Self"):
                continue
            if reached in wrapped:
                n += 1
    return n


def main_scan(root: pathlib.Path) -> int:
    unwrapped = allowed = wrapped = 0
    bad: list[str] = []
    for f in sorted(root.rglob("*.rs")):
        rel = str(f.relative_to(root))
        text = f.read_text(encoding="utf-8", errors="replace")
        if "self_test" not in text:
            continue
        mine = own_module(rel)
        wrapped += wrapped_count(text, rel)
        for line_no, reached, verb in findings_for(text, rel):
            unwrapped += 1
            key = (mine, reached, verb)
            if key in ALLOWED:
                allowed += 1
                continue
            bad.append(
                f"kernel/src/{rel.replace(chr(92), '/')}:{line_no}: "
                f"{mine}'s self-test calls {reached}::{verb}() and nothing moves "
                f"{reached}'s state aside"
            )

    if bad:
        print("check-selftest-reach: FINDING(S)")
        for b in bad:
            print(f"  {b}")
        print()
        print("A suite that clears another module's state destroys it on a live")
        print("machine and then reports success.  At boot these tables are usually")
        print("empty, so this will not fail a boot test -- that is why it needs a")
        print("static check.  Fix it one of two ways:")
        print()
        print("  * give the reached module a `pub(crate) fn with_pristine_state`")
        print("    and compose it in this suite's `pub fn self_test`, the way")
        print("    `svcstart::self_test` composes `servicemgr::with_pristine_state`; or")
        print("  * add the triple to ALLOWED in this script with the reason it is")
        print("    safe -- which must be about the reached function, not about how")
        print("    unlikely the suite is to run.")
        return 1

    print(
        f"check-selftest-reach: OK ({wrapped + unwrapped} destructive reach(es) from "
        f"self-tests; {wrapped} moved aside by the reached module's wrapper, "
        f"{allowed} allowed with a reason)"
    )
    return 0


def self_test() -> int:
    NL = chr(10)
    cases: list[tuple[str, str, str, list[tuple[int, str, str]]]] = [
        (
            "an unwrapped reach is a finding",
            "sockact.rs",
            NL.join([
                "pub fn self_test() -> R {",
                "    crate::fs::servicemgr::clear_all();",
                "}",
            ]),
            [(2, "servicemgr", "clear_all")],
        ),
        (
            "composing the reached module's wrapper clears it",
            "svcstart.rs",
            NL.join([
                "pub fn self_test() -> R {",
                "    servicemgr::with_pristine_state(|| self_test_inner())",
                "}",
                "fn self_test_inner() -> R {",
                "    servicemgr::clear_all();",
                "}",
            ]),
            [],
        ),
        (
            "a module clearing its OWN state is not a reach",
            "servicemgr.rs",
            NL.join([
                "pub fn self_test() -> R {",
                "    servicemgr::clear_all();",
                "}",
            ]),
            [],
        ),
        (
            "production code is out of scope -- only self_test* bodies count",
            "linux.rs",
            NL.join([
                "pub fn sys_sethostname() -> R {",
                "    crate::fs::nameservice::init_defaults();",
                "}",
            ]),
            [],
        ),
        (
            "a call in a comment is not a call",
            "sockact.rs",
            NL.join([
                "pub fn self_test() -> R {",
                "    // crate::fs::servicemgr::clear_all();",
                "}",
            ]),
            [],
        ),
        (
            "a call in a string literal is not a call",
            "sockact.rs",
            NL.join([
                "pub fn self_test() -> R {",
                '    serial_println!("call servicemgr::clear_all() to wipe");',
                "}",
            ]),
            [],
        ),
        (
            "a nested helper inside a self-test is still inside it",
            "sockact.rs",
            NL.join([
                "pub fn self_test() -> R {",
                "    fn case() {",
                "        eventlog::clear();",
                "    }",
                "    case();",
                "}",
            ]),
            [(3, "eventlog", "clear")],
        ),
        (
            "mod.rs takes its directory's name, so fs/ext4/mod.rs is `ext4`",
            "fs/ext4/mod.rs",
            NL.join([
                "pub fn self_test() -> R {",
                "    ext4::clear_all();",
                "}",
            ]),
            [],
        ),
        (
            "a verb not on the list is invisible -- the documented limitation",
            "sockact.rs",
            NL.join([
                "pub fn self_test() -> R {",
                "    servicemgr::obliterate_everything();",
                "}",
            ]),
            [],
        ),
        (
            "two reaches on one line are both reported",
            "sockact.rs",
            NL.join([
                "pub fn self_test() -> R {",
                "    a::clear(); b::purge();",
                "}",
            ]),
            [(2, "a", "clear"), (2, "b", "purge")],
        ),
    ]

    failures = 0

    # --- the case that is not synthetic -------------------------------------
    #
    # Everything above proves the logic works on strings written for it. None of
    # it would notice if `rust_scopes` stopped recognising the shapes this kernel
    # actually uses, if `self_test_inner` were renamed tree-wide, or if the scan
    # stopped opening files. See known-issues ->
    # TD-A-A-WIRED-GATE-CAN-GRADE-ONE-LINE-AND-LOOK-LIKE-IT-GRADES-A-SUBSYSTEM,
    # where two wired gates reported a clean tree while the code they are named
    # after was deliberately broken.
    #
    # So: take the real file, confirm the gate is quiet on it, inject one reach
    # into its self-test body, and require the gate to find exactly that. In
    # memory -- the file is never written.
    # Script-relative first: this file is in `scripts/`, so its grandparent is the
    # repository root by construction, whatever the caller's working directory is.
    # The CWD-relative form is kept as a fallback for a tree laid out differently.
    # This case FAILS rather than skips when the subject is missing, so it must not
    # depend on an ambient assumption: the cost of being wrong is a red tree for
    # every lane, raised by a self-test rather than by a defect.
    here = pathlib.Path(__file__).resolve().parent.parent
    subject = here / "kernel/src/sockact.rs"
    if not subject.is_file():
        subject = pathlib.Path("kernel/src/sockact.rs")
    if not subject.is_file():
        print("  SKIP  mutation case: kernel/src/sockact.rs not found")
        print("        (run from the repository root; this case is the only one")
        print("         that proves the gate is still attached to the tree)")
        failures += 1
    else:
        real = subject.read_text(encoding="utf-8", errors="replace")
        before = findings_for(real, "sockact.rs")
        if before:
            failures += 1
            print("  FAIL  mutation case: the real sockact.rs already has findings")
            print(f"        {before}")
        else:
            # Inject after the first line that is inside a self_test* scope, so
            # the injection lands where a real regression would.
            lines = real.splitlines()
            stacks = rust_scopes.scope_stack_per_line(lines)
            at = next(
                (
                    i
                    for i, st in enumerate(stacks)
                    if any(s.name and s.name.startswith("self_test") for s in st)
                ),
                None,
            )
            if at is None:
                failures += 1
                print("  FAIL  mutation case: no self_test scope found in the real file")
                print("        (that alone means this gate grades nothing here)")
            else:
                lines.insert(at, "    crate::eventlog::clear();")
                after = findings_for(NL_.join(lines), "sockact.rs")
                hit = [f for f in after if f[1:] == ("eventlog", "clear")]
                if len(hit) == 1 and len(after) == 1:
                    print("  ok    mutation case: the real sockact.rs, one reach injected")
                else:
                    failures += 1
                    print("  FAIL  mutation case: injected one reach into the real file")
                    print(f"        want exactly one eventlog::clear finding, got {after}")

    for name, rel, src, want in cases:
        got = findings_for(src, rel)
        if got == want:
            print(f"  ok    {name}")
        else:
            failures += 1
            print(f"  FAIL  {name}")
            print(f"        want {want}")
            print(f"        got  {got}")

    print(
        f"check-selftest-reach: self-test {'passed' if not failures else 'FAILED'} "
        f"({failures} failure(s), {len(cases)} synthetic case(s) + 1 against the real tree)"
    )
    return 1 if failures else 0


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--self-test", "--selftest", action="store_true",
                    help="run this checker's own cases and exit")
    ap.add_argument("--root", default="kernel/src", help="tree to scan")
    args = ap.parse_args(argv)
    if args.self_test:
        return self_test()
    return main_scan(pathlib.Path(args.root))


if __name__ == "__main__":
    sys.exit(main())

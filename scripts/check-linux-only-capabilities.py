#!/usr/bin/env python3
"""A kernel capability reachable from the Linux ABI table and from no native one.

WHY THIS EXISTS
---------------
Three times, the kernel had an implementation and only the Linux-compatibility
syscall table could reach it. Native libc called a number that did not exist, or
an older one that did something narrower, and reported success:

    setgroups          reported success and changed nothing
    pty slave read     read the console instead, and hung a boot test for 2h
    alarm / setitimer  proc/itimer.rs was there; native libc armed nothing

Lane B found all three by tripping over a *userspace symptom*, and asked lane A
for the list rather than a fourth bug report
(`notice-all-b-20260909T183721Z`). Their observation is the reason this is a
gate and not a one-off: the libc side looks finished in all three cases --
careful validation, real tests -- so nothing invites suspicion. The asymmetry is
invisible from either side alone, and visible in one pass from here.

All three are fixed (SYS_PROCESS_SETGROUPS 1067, SYS_PTY_SLAVE_READ 872,
SYS_ITIMER_SET/GET 1069/1070), which is what makes them useful: they are known
negatives, and a version of this check that reports them is measuring something
other than what it claims. `--self-test` asserts exactly that.

WHY IT IS A RATCHET AND NOT A GATE
----------------------------------
Fourteen modules are Linux-only as this is written, and thirteen of them are
deliberate -- `ipc::epoll` has no native syscall number on purpose, because this
system uses channels. Failing outright would block all three lanes over state
none of them created, and a gate that does that is a gate that gets bypassed.
So the known set is pinned in `BASELINE` with a reason each, and this fails when
the set *changes*:

  - a module that is Linux-only and not pinned  -- the ratchet slipping;
  - a pinned module that is no longer Linux-only -- prune it, because a
    baseline nobody prunes stops describing the tree it exempts.

This is the sibling of `check-gates-are-wired.py` and follows the same rule.

METHOD
------
For each syscall table, collect the kernel **modules** its handlers reach:

    native   kernel/src/syscall/dispatch.rs  handlers[SYS_X as usize] = Some(handlers::sys_y)
    linux    kernel/src/syscall/linux.rs     nr::FOO => sys_bar(args)

A call counts whether written `crate::ipc::futex::futex_wait(...)` or as a bare
`futex::futex_wait(...)` resolved through the file's `use` declarations. Then
difference the two sets.

TWO EARLIER VERSIONS WERE WRONG AND BOTH LOOKED AUTHORITATIVE
-------------------------------------------------------------
Kept because the shape recurs, not as history:

  * **Full transitive closure** over calls to other functions in the same file
    reported 238 capabilities with `landlock_restrict_self` in nearly every
    line. Following local helpers makes everything reachable from everything.

  * **Function granularity** reported SETITIMER and ALARM, because the Linux
    handler calls `proc::itimer::timeval_to_ns` and the native ABI deliberately
    takes nanoseconds in registers (design-decisions.md 925). It was flagging
    ABI-shape helpers as missing capabilities.

  * A third, subtler one: matching only `crate::`-prefixed calls missed
    `futex::futex_wait(...)` in a native handler, so `ipc::futex` was reported
    as Linux-only while ten native futex syscalls were wired. `use`-alias
    resolution is what removes that class.

LIMITS, which are why each finding wants a human
------------------------------------------------
Rust is not parsed; calls are matched textually. So a capability reached through
a trait object, a function pointer or a macro is invisible; a name in a comment
is counted; and "no native handler calls it" is not "no native caller exists",
since the scheduler and interrupt paths call subsystems with no syscall
involved. A finding here is a question, not a verdict.
"""

from __future__ import annotations

import argparse
import pathlib
import re
import sys
from collections import defaultdict

REPO = pathlib.Path(__file__).resolve().parent.parent
DISPATCH = "kernel/src/syscall/dispatch.rs"
LINUX = "kernel/src/syscall/linux.rs"
HANDLERS = "kernel/src/syscall/handlers.rs"

NATIVE_ARM = re.compile(
    r"handlers\[\s*(SYS_[A-Z0-9_]+)\s+as\s+usize\s*\]\s*=\s*Some\(\s*handlers::([a-z0-9_]+)"
)
LINUX_ARM = re.compile(r"nr::([A-Z0-9_]+)\s*=>\s*([a-z0-9_:]+)\s*\(")

CRATE_CALL = re.compile(r"\bcrate::([a-z0-9_]+(?:::[a-z0-9_]+)+)\s*\(")
QUAL_CALL = re.compile(r"\b([a-z][a-z0-9_]*(?:::[a-z0-9_]+)+)\s*\(")
USE_SIMPLE = re.compile(r"^\s*(?:pub\s+)?use\s+crate::([a-z0-9_:]+);", re.M)
USE_GROUP = re.compile(r"^\s*(?:pub\s+)?use\s+crate::([a-z0-9_:]+)::\{([^}]*)\};", re.M)

# A call into `syscall::*` is the layer talking to itself, not a capability.
NOT_A_CAPABILITY = {"syscall"}

# Linux syscalls whose handlers must NOT appear in the report. Each has a wired
# native number; if one is named, the difference is being computed wrongly and
# nothing else this prints can be trusted.
CALIBRATION = (
    "SETGROUPS",
    "SETITIMER",
    "GETITIMER",
    "ALARM",
    "PTY_SLAVE_READ",
    "FUTEX",
    "FUTEX_REQUEUE",
)

# Modules that are Linux-only today, with the reason each is allowed to be.
# This list may only SHRINK. Adding to it needs the reason, not just the name.
BASELINE: dict[str, str] = {
    # --- one polymorphic syscall, not eight findings ------------------------
    # `pidfd_getfd` dups a descriptor of *any* type, so it reaches every fd
    # module's `dup`. There are zero native SYS_*PIDFD* constants: borrowing a
    # descriptor out of another process is a Linux mechanism, and this system's
    # answer is capability transfer over a channel.
    "drm::card_fd": "reached only by pidfd_getfd's dup; no native pidfd exists",
    "evdev_fd": "reached only by pidfd_getfd's dup; no native pidfd exists",
    "ipc::alsa_pcm": "reached only by pidfd_getfd's dup; no native pidfd exists",
    # --- Linux-flavoured descriptor types, deliberate -----------------------
    # Zero native syscall numbers each, on purpose: readiness and notification
    # here are channels, not descriptors you poll.
    "ipc::epoll": "no native epoll by design -- channels, not a readiness fd",
    "ipc::eventfd": "no native eventfd by design -- channels carry wakeups",
    "ipc::inotify": "no native inotify by design -- fs watches are a service",
    "ipc::signalfd": "no native signalfd by design -- signals are not fds here",
    "ipc::memfd": "no native memfd by design -- anonymous memory is shm + caps",
    "ipc::pipe": "native pipes exist; only fcntl's F_GETPIPE_SZ/F_SETPIPE_SZ "
    "and pidfd_getfd reach this module and nothing native does",
    # --- a compatibility shim over a stack native code reaches elsewhere ----
    # 46 native net handlers exist; they reach other modules. linux.rs carries
    # its own BSD-sockets layer, which is the point of a compatibility table.
    "net::socket": "linux.rs's own BSD-sockets shim; native net uses SYS_NET_*",
    "net::netstack_client": "same shim; native net reaches the stack elsewhere",
    # --- plumbing, not a capability -----------------------------------------
    "mm::page_table": "vmsplice/getrusage read page tables; not a capability a "
    "native program would ask for by number",
    # --- Linux-specific thread machinery ------------------------------------
    "proc::thread_clone": "rseq, robust futex lists and prctl are Linux TLS/futex "
    "machinery with no native equivalent intended",
    # --- OPEN: this one is a question, not a decision -----------------------
    # There is NO native syscall for the hostname or domain name at all --
    # verified by grepping number.rs, where the only matches are the unrelated
    # SYS_DMA_DOMAIN_CREATE/DESTROY. sethostname, setdomainname and uname reach
    # fs::nameservice and nothing native does. Whether that is a defect depends
    # on whether our libc's gethostname goes through a native number or the
    # Linux table, which is lane B's to answer; asked 2026-09-10. If it is
    # native, this is instance four with the same signature as setgroups.
    "fs::nameservice": "OPEN QUESTION to lane B (2026-09-10): no native hostname "
    "syscall exists at all. Not known to be deliberate -- remove this entry when "
    "answered, in either direction",
}


def functions(src: str) -> dict[str, str]:
    """fn name -> body text, by brace matching from each `fn name(`."""
    out: dict[str, str] = {}
    for m in re.finditer(r"\bfn\s+([a-z_][a-z0-9_]*)\s*[(<]", src):
        i = src.find("{", m.end())
        if i < 0:
            continue
        depth, j = 0, i
        while j < len(src):
            if src[j] == "{":
                depth += 1
            elif src[j] == "}":
                depth -= 1
                if depth == 0:
                    break
            j += 1
        out.setdefault(m.group(1), src[i : j + 1])
    return out


def use_aliases(src: str) -> dict[str, str]:
    """Last path segment -> full crate-relative path, from `use` declarations.

    Without this, `futex::futex_wait(...)` in a native handler is invisible and
    `ipc::futex` is reported as Linux-only while ten native futex syscalls are
    wired. That was a real false positive, removed by this resolution.
    """
    out: dict[str, str] = {}
    for path in USE_SIMPLE.findall(src):
        out[path.split("::")[-1]] = path
    for base, names in USE_GROUP.findall(src):
        for n in names.split(","):
            n = n.strip().split(" as ")[0].strip()
            if n and n != "self":
                out[n] = base + "::" + n
    return out


def modules_reached(
    entry: str, bodies: dict[str, str], alias: dict[str, str]
) -> set[str]:
    """The kernel modules one handler's own body calls into.

    Depth one on purpose: following local helpers makes the closure useless (it
    reported 238 capabilities), and a handler that defers its whole job to a
    helper in the same file is rare enough to be worth missing for an output
    somebody will actually read.
    """
    body = bodies.get(entry, "")
    out: set[str] = set()
    for path in CRATE_CALL.findall(body):
        parts = path.split("::")
        if parts[0] not in NOT_A_CAPABILITY:
            out.add("::".join(parts[:-1]))
    for path in QUAL_CALL.findall(body):
        parts = path.split("::")
        if parts[0] == "crate" or parts[0] not in alias:
            continue
        full = alias[parts[0]]
        if len(parts) > 2:
            full += "::" + "::".join(parts[1:-1])
        if full.split("::")[0] not in NOT_A_CAPABILITY:
            out.add(full)
    return out


def analyse(repo: pathlib.Path) -> tuple[set[str], dict[str, set[str]]]:
    """(modules native handlers reach, {linux syscall: modules only it reaches})."""
    dsrc = (repo / DISPATCH).read_text(encoding="utf-8", errors="replace")
    lsrc = (repo / LINUX).read_text(encoding="utf-8", errors="replace")
    hsrc = (repo / HANDLERS).read_text(encoding="utf-8", errors="replace")

    native_arms = NATIVE_ARM.findall(dsrc)
    linux_arms = LINUX_ARM.findall(lsrc)
    if not native_arms or not linux_arms:
        # Not a finding against the tree: this check cannot see the tables.
        print(
            f"check-linux-only-capabilities: parsed {len(native_arms)} native and "
            f"{len(linux_arms)} linux dispatch arms, which cannot be right -- a "
            "table changed shape and this check is reading nothing. Fix the "
            "patterns; do not trust a clean result from this state.",
            file=sys.stderr,
        )
        raise SystemExit(2)

    hb, lb = functions(hsrc), functions(lsrc)
    ha, la = use_aliases(hsrc), use_aliases(lsrc)

    native: set[str] = set()
    for _sys, fn in native_arms:
        native |= modules_reached(fn, hb, ha)

    only: dict[str, set[str]] = {}
    for nr_name, fn in linux_arms:
        gap = modules_reached(fn.split("::")[-1], lb, la) - native
        if gap:
            only[nr_name] = gap
    return native, only


def calibration_failures(only: dict[str, set[str]]) -> list[str]:
    return [c for c in CALIBRATION if c in only]


def self_test(repo: pathlib.Path) -> int:
    """The seven known negatives, plus that the baseline can be wrong.

    The calibration half is the load-bearing one: every wrong version of this
    check produced a long, well-formatted, authoritative-looking report, and the
    only thing that distinguished them from the right one was whether a syscall
    with a wired native number showed up in it.
    """
    failures = 0
    _native, only = analyse(repo)

    bad = calibration_failures(only)
    if bad:
        print(
            "selftest FAIL: " + ", ".join(bad) + " reported as Linux-only, but each "
            "has a wired native number. The difference is being computed wrongly.",
            file=sys.stderr,
        )
        failures += 1

    # A baseline entry that is not a string reason is a name somebody added
    # without saying why, which is how an exemption list rots into a deny-list.
    for mod, reason in BASELINE.items():
        if not isinstance(reason, str) or len(reason.strip()) < 20:
            print(
                f"selftest FAIL: BASELINE['{mod}'] has no usable reason. An "
                "exemption without a reason cannot be reviewed or pruned.",
                file=sys.stderr,
            )
            failures += 1

    # And that the comparison can actually refuse: a module invented here must
    # be reported as unpinned.
    live = {m for mods in only.values() for m in mods}
    invented = "not_a_real_module::nope"
    if invented in (live | set(BASELINE)):
        print("selftest FAIL: the invented module name is real", file=sys.stderr)
        failures += 1
    else:
        if not ({invented} - set(BASELINE)):
            print(
                "selftest FAIL: an unpinned module is not detected as unpinned",
                file=sys.stderr,
            )
            failures += 1

    if failures:
        print(f"selftest: {failures} case(s) FAILED", file=sys.stderr)
        return 1
    print(
        f"check-linux-only-capabilities --self-test: OK "
        f"({len(CALIBRATION)} known-native syscalls absent, "
        f"{len(BASELINE)} baselined modules each with a reason)"
    )
    return 0


def main() -> int:
    ap = argparse.ArgumentParser(description="capabilities only the Linux ABI reaches")
    ap.add_argument("--self-test", "--selftest", action="store_true", dest="selftest")
    ap.add_argument(
        "--list",
        action="store_true",
        help="print every Linux-only module and the syscalls reaching it",
    )
    args = ap.parse_args()

    if args.selftest:
        return self_test(REPO)

    _native, only = analyse(REPO)

    bad = calibration_failures(only)
    if bad:
        print(
            "check-linux-only-capabilities: " + ", ".join(bad) + " reported as "
            "Linux-only, but each has a wired native number. This check is "
            "broken, not the tree -- its own --self-test says the same.",
            file=sys.stderr,
        )
        return 2

    by_mod: dict[str, list[str]] = defaultdict(list)
    for nr_name, mods in only.items():
        for mod in mods:
            by_mod[mod].append(nr_name)

    if args.list:
        for mod in sorted(by_mod):
            pin = BASELINE.get(mod)
            print(f"{mod}{'' if pin else '   (NOT pinned)'}")
            print(f"    via {', '.join(sorted(by_mod[mod]))}")
            if pin:
                print(f"    pinned: {pin}")
        return 0

    live = set(by_mod)
    pinned = set(BASELINE)
    unpinned = sorted(live - pinned)
    stale = sorted(pinned - live)

    for mod in unpinned:
        print(f"{mod}: reachable from the Linux ABI table and from no native syscall")
        print(f"    via {', '.join(sorted(by_mod[mod])[:8])}")
    for mod in stale:
        print(f"{mod}: pinned as Linux-only but native code now reaches it")
        print(f"    remove it from BASELINE in {pathlib.Path(__file__).name}")

    if unpinned or stale:
        print()
        print(
            f"{len(unpinned)} unpinned, {len(stale)} stale. A new entry is a "
            "question, not a verdict: decide whether native code should be able "
            "to ask for this, give it a syscall number if so, and pin it with the "
            "reason if not. See this file's header for what the check cannot see."
        )
        return 1

    print(
        f"check-linux-only-capabilities: OK ({len(live)} Linux-only module(s), "
        f"all {len(pinned)} baselined, none stale)"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())

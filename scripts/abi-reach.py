#!/usr/bin/env python3
"""Which kernel capabilities can the Linux-ABI table reach that no native number can?

Lane B has now tripped over the same defect four times: `setgroups` (returned -1/ENOSYS),
the pty slave read (hung their boot test), `alarm`/`setitimer` (reported success and armed
nothing), and `gethostname`/`sethostname` (returned 0 and changed a process-local buffer).
Every time the kernel already had the implementation and only the Linux-ABI table could
reach it, so the libc side looked finished -- validated, tested, and wrong. Each was found
by tripping over the userspace symptom, which is the expensive way.

Their suggestion, and it is worth more than the two requests it arrived with: *"A list of
handlers reachable from the Linux-ABI table but from no native number would find the
remainder in one pass. That is your table; I will consume whatever it says."*

WHAT THIS IS NOT. Not a gate, deliberately. Every entry it prints is a candidate, not a
defect: plenty of Linux calls have no native equivalent *by design* -- the native ABI is
not a clone of Linux's -- so a blocking version would need an allowlist the size of its
own output on day one, which is the shape of check that gets switched off. It prints a
ranked report and exits 0 whatever it finds. `--strict` exits 1 on a non-empty report for
anyone who decides otherwise later.

HOW IT READS THE TREE. Three facts, each taken from the file that owns it:

  * `linux.rs` defines its own shim per Linux number and dispatches `nr::X => sys_y(args)`.
    Those `sys_y` are NOT the native handlers -- they are translation shims, and there are
    373 of them.
  * `dispatch.rs` registers native handlers by table assignment,
    `handlers[SYS_X as usize] = Some(handlers::sys_y)`. A handler defined in `handlers.rs`
    and never assigned is unreachable natively, which is worth knowing on its own and is
    reported separately.
  * So the comparison is not between the two tables' function names -- they share almost
    none -- but between what the two sets of functions CALL. A `crate::a::b::c(...)`
    reached from a dispatched Linux shim and from no registered native handler is the
    signature of all four instances above.

DEPTH, STATED BECAUSE IT BOUNDS THE ANSWER. One level: the calls appearing directly in a
shim or handler body. A capability reached only through a private helper shared by both
tables will not appear, and one reached through a helper the Linux side alone uses will
appear as the helper rather than as the capability. Transitive closure would be more
precise and much harder to read; this pass exists to produce a list a human can work
through.

Reads Rust through `rustscan`, so a comment mentioning a call, a `#[cfg(test)]` module
exercising it, and a brace inside a string literal cannot move the answer.
"""

from __future__ import annotations

import argparse
import pathlib
import re
import sys
import tempfile

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
import rustscan  # noqa: E402

ROOT = pathlib.Path(__file__).resolve().parent.parent
SYSCALL = ROOT / "kernel" / "src" / "syscall"

#: `fn sys_foo(` at the start of a line, `pub` or not.
FN_RE = re.compile(r"^(?:pub(?:\([a-z:]+\))? )?(?:unsafe )?fn (sys_[a-z0-9_]+)\s*\(", re.M)
#: `nr::NAME => sys_foo(` -- a Linux number wired to its shim.
LINUX_ARM_RE = re.compile(r"\bnr::([A-Z0-9_]+)\s*=>\s*(sys_[a-z0-9_]+)\s*\(")
#: `handlers[SYS_NAME as usize] = Some(handlers::sys_foo)` -- a native registration.
NATIVE_REG_RE = re.compile(
    r"handlers\[\s*(SYS_[A-Z0-9_]+)\s+as\s+usize\s*\]\s*=\s*Some\(\s*handlers::(sys_[a-z0-9_]+)")
#: A reference to another kernel module's function. The `crate::` prefix is required, so
#: local helpers and `self::` calls are not mistaken for capabilities.
#:
#: Deliberately does NOT require a trailing open-paren. A capability passed as a
#: FUNCTION POINTER is still reached, and requiring the paren missed exactly that: the
#: native hostname handler is `uts_name_set(args, crate::fs::nameservice::set_hostname)`,
#: so `set_hostname` was scored unreachable natively and reported as a Linux-only
#: capability when a native number had reached it all along. Found by checking the report
#: against an instance whose answer was already known -- the only reason it was found.
#:
#: The last segment must be lowercase, so `crate::foo::SOME_CONST` and type paths like
#: `crate::fs::vfs::Vfs` do not match. A `use crate::a::b::c;` inside a body DOES match,
#: and should: importing a function in order to call it unqualified is reaching it.
CRATE_CALL_RE = re.compile(r"\bcrate::((?:[a-z_][a-z0-9_]*::)+[a-z_][a-z0-9_]*)\b")

#: Module paths that are the syscall boundary's own plumbing rather than kernel
#: capabilities. Listed rather than filtered by frequency so the reason survives a reader.
PLUMBING = frozenset({
    "syscall", "arch", "mm::uaccess", "proc::current", "sched::current",
})


def read(path: pathlib.Path) -> str:
    return rustscan.production_only(path.read_text(encoding="utf-8", errors="replace"))


def is_plumbing(call: str) -> bool:
    """Whether `call` is syscall-boundary plumbing rather than a kernel capability."""
    head = call.rsplit("::", 1)[0]
    return head in PLUMBING or call.split("::", 1)[0] in PLUMBING


def fn_calls(text: str) -> dict[str, set[str]]:
    """Per `fn sys_*` in `text`, the `crate::` calls appearing directly in its body."""
    out: dict[str, set[str]] = {}
    for m in FN_RE.finditer(text):
        body = rustscan.fn_body(text, m.start())
        if body is None:
            continue
        calls = {c for c in CRATE_CALL_RE.findall(body) if not is_plumbing(c)}
        out.setdefault(m.group(1), set()).update(calls)
    return out


def survey(syscall_dir: pathlib.Path = SYSCALL) -> dict:
    linux_text = read(syscall_dir / "linux.rs")
    handlers_text = read(syscall_dir / "handlers.rs")
    dispatch_text = read(syscall_dir / "dispatch.rs")

    linux_arms = {shim: nr for nr, shim in LINUX_ARM_RE.findall(linux_text)}
    native_regs = {fn: nr for nr, fn in NATIVE_REG_RE.findall(dispatch_text)}

    linux_bodies = fn_calls(linux_text)
    handler_bodies = fn_calls(handlers_text)

    # Only DISPATCHED shims count: an unreferenced shim in linux.rs reaches nothing.
    linux_reach: dict[str, set[str]] = {}
    for shim in linux_arms:
        for call in linux_bodies.get(shim, ()):
            linux_reach.setdefault(call, set()).add(shim)

    # Only REGISTERED handlers count, for the same reason in the other direction.
    native_reach: set[str] = set()
    for fn in native_regs:
        native_reach |= handler_bodies.get(fn, set())

    only_linux = {c: sorted(s) for c, s in linux_reach.items() if c not in native_reach}

    # MODULE granularity, which is the question actually asked. A single function being
    # Linux-only is usually benign: `proc::itimer::timeval_to_ns` converts a `timeval`
    # to nanoseconds, which only the Linux ABI needs because the native call takes
    # nanoseconds directly -- and `proc::itimer::set_real` IS reached by SYS_ITIMER_SET.
    # Reporting that function as Linux-only is true and led me to tell lane B their
    # itimer request was still open when it had been done the same day they filed it.
    # A module where NOTHING is natively reachable is the real signal: that is a
    # capability with no native door, which is what all four known instances were.
    def module_of(call):
        return call.rsplit("::", 1)[0]

    native_modules = {module_of(c) for c in native_reach}
    only_linux_modules = {}
    for call, shims in linux_reach.items():
        mod = module_of(call)
        if mod not in native_modules:
            only_linux_modules.setdefault(mod, set()).update(shims)
    only_linux_modules = {m: sorted(v) for m, v in only_linux_modules.items()}
    unregistered = sorted(set(handler_bodies) - set(native_regs))
    return {
        "linux_arms": linux_arms,
        "native_regs": native_regs,
        "only_linux": only_linux,
        "unregistered_handlers": unregistered,
        "only_linux_modules": only_linux_modules,
        "linux_reach_total": len(linux_reach),
        "native_reach_total": len(native_reach),
    }


def report(strict: bool = False) -> int:
    s = survey()
    print(f"abi-reach: {len(s['linux_arms'])} dispatched Linux shim(s), "
          f"{len(s['native_regs'])} registered native handler(s).")
    print(f"  Linux shims reach {s['linux_reach_total']} distinct kernel call(s); "
          f"native handlers reach {s['native_reach_total']}.")
    print()

    mods = s["only_linux_modules"]
    if mods:
        print(f"  *** {len(mods)} MODULE(S) that no registered native handler reaches at"
              " all. This is the headline: a module with no native door is what all four"
              " known instances were.")
        print()
        for mod, shims in sorted(mods.items(), key=lambda kv: (-len(kv[1]), kv[0])):
            names = ", ".join(shims[:4]) + (f" +{len(shims) - 4}" if len(shims) > 4 else "")
            print(f"    {mod}")
            print(f"        via {names}")
        print()
    else:
        print("  Every module a Linux shim reaches is also reached by some native")
        print("  handler. Individual functions below may still be Linux-only, which is")
        print("  usually a data-shape conversion the native ABI does not need.")
        print()

    only = s["only_linux"]
    if not only:
        print("  Nothing is reachable from the Linux table alone. (One level deep --")
        print("  see the module docstring for what that does and does not cover.)")
    else:
        print(f"  *** {len(only)} kernel call(s) reachable from a Linux shim and from NO")
        print("  registered native handler. Candidates, not defects: the native ABI is not")
        print("  a clone of Linux's, so some have no native number by design.")
        print()
        # Most-referenced first: a call several Linux numbers reach is likelier to be a
        # capability someone wants natively than a one-off translation detail.
        for call, shims in sorted(only.items(), key=lambda kv: (-len(kv[1]), kv[0])):
            names = ", ".join(shims[:4]) + (f" +{len(shims) - 4}" if len(shims) > 4 else "")
            print(f"    {call}")
            print(f"        via {names}")

    if s["unregistered_handlers"]:
        print()
        print(f"  Also: {len(s['unregistered_handlers'])} handler(s) defined in handlers.rs")
        print("  and assigned no native number in dispatch.rs -- unreachable natively.")
        for fn in s["unregistered_handlers"][:20]:
            print(f"    {fn}")
        if len(s["unregistered_handlers"]) > 20:
            print(f"    ... and {len(s['unregistered_handlers']) - 20} more")

    return 1 if (strict and only) else 0


def _write_tables(d: pathlib.Path, linux: str, handlers: str, dispatch: str) -> dict:
    (d / "linux.rs").write_text(linux, encoding="utf-8", newline="")
    (d / "handlers.rs").write_text(handlers, encoding="utf-8", newline="")
    (d / "dispatch.rs").write_text(dispatch, encoding="utf-8", newline="")
    return survey(d)


def self_test() -> int:
    """Grade the parser against synthetic tables and against the real tree."""
    cases: list[tuple[str, object, object]] = []
    NL = chr(10)

    linux = NL.join([
        "fn sys_alarm(args: &SyscallArgs) -> SyscallResult {",
        "    crate::proc::itimer::setitimer(args.a0)",
        "}",
        "fn sys_orphan(args: &SyscallArgs) -> SyscallResult {",
        "    crate::nobody::calls_me(args.a0)",
        "}",
        "fn sys_read(args: &SyscallArgs) -> SyscallResult {",
        "    crate::fs::vfs::read(args.a0)",
        "}",
        "pub fn dispatch(n: u64) -> SyscallResult {",
        "    match n {",
        "        nr::ALARM => sys_alarm(args),",
        "        nr::READ => sys_read(args),",
        "        _ => Err(()),",
        "    }",
        "}",
    ])
    handlers = NL.join([
        "pub fn sys_read_native(args: &SyscallArgs) -> SyscallResult {",
        "    crate::fs::vfs::read(args.a0)",
        "}",
        "pub fn sys_never_wired(args: &SyscallArgs) -> SyscallResult {",
        "    crate::fs::vfs::write(args.a0)",
        "}",
    ])
    dispatch = "    handlers[SYS_READ as usize] = Some(handlers::sys_read_native);"

    with tempfile.TemporaryDirectory() as td:
        s = _write_tables(pathlib.Path(td), linux, handlers, dispatch)

    cases += [
        ("a call reached only from a dispatched Linux shim is reported",
         "proc::itimer::setitimer" in s["only_linux"], True),
        ("and it names the shim that reaches it",
         s["only_linux"].get("proc::itimer::setitimer"), ["sys_alarm"]),
        ("a call both tables reach is NOT reported",
         "fs::vfs::read" in s["only_linux"], False),
        ("an UNDISPATCHED Linux shim reaches nothing -- no number, no reach",
         "nobody::calls_me" in s["only_linux"], False),
        ("a handler assigned no native number is reported separately",
         s["unregistered_handlers"], ["sys_never_wired"]),
        ("a call reached only from an unregistered handler does not count as native",
         "fs::vfs::write" in s["only_linux"], False),
    ]

    # A comment naming a call must not count as reaching it -- the trap `rustscan` exists
    # for, asserted here rather than assumed of the import.
    commented = NL.join([
        "fn sys_x(args: &SyscallArgs) -> SyscallResult {",
        "    // crate::ghost::never_called(args.a0)",
        "    Ok(0)",
        "}",
        "pub fn dispatch(n: u64) { match n { nr::X => sys_x(args), _ => () } }",
    ])
    with tempfile.TemporaryDirectory() as td:
        g = _write_tables(pathlib.Path(td), commented, "", "")
    cases.append(("a commented-out call is not a reach",
                  "ghost::never_called" in g["only_linux"], False))

    # A capability the native side reaches only as a FUNCTION POINTER must count as
    # reached. Requiring a trailing open-paren made the real report claim set_hostname
    # was Linux-only when SYS_HOSTNAME_SET had reached it all along.
    ptr_linux = NL.join([
        "fn sys_sethostname(args: &SyscallArgs) -> SyscallResult {",
        "    crate::fs::nameservice::set_hostname(args.a0)",
        "}",
        "pub fn dispatch(n: u64) { match n { nr::SETHOSTNAME => sys_sethostname(args), _ => () } }",
    ])
    ptr_handlers = NL.join([
        "pub fn sys_hostname_set(args: &SyscallArgs) -> SyscallResult {",
        "    uts_name_set(args, crate::fs::nameservice::set_hostname)",
        "}",
    ])
    with tempfile.TemporaryDirectory() as td:
        ptr = _write_tables(
            pathlib.Path(td), ptr_linux, ptr_handlers,
            "    handlers[SYS_HOSTNAME_SET as usize] = Some(handlers::sys_hostname_set);")
    cases.append(("a capability reached natively as a function pointer is not Linux-only",
                  "fs::nameservice::set_hostname" in ptr["only_linux"], False))

    # Against the real tree: the shapes must be found at all, or the regexes have drifted
    # from the files and every later conclusion is vacuous rather than reassuring.
    real = survey()
    cases += [
        ("the real linux.rs yields a substantial dispatched shim set",
         len(real["linux_arms"]) > 300, True),
        ("the real dispatch.rs yields a substantial native registration set",
         len(real["native_regs"]) > 300, True),
        ("the real handlers.rs bodies parse to some kernel calls",
         real["native_reach_total"] > 50, True),
    ]

    failures = 0
    for name, got, want in cases:
        if got == want:
            print(f"  ok    {name}")
        else:
            failures += 1
            print(f"  FAIL  {name}")
            print(f"        want {want!r}")
            print(f"        got  {got!r}")
    print(f"abi-reach: self-test {'passed' if not failures else 'FAILED'} "
          f"({failures} failure(s), {len(cases)} case(s))")
    return 1 if failures else 0


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--self-test", "--selftest", action="store_true",
                    help="grade the parser against synthetic tables and the real tree")
    ap.add_argument("--strict", action="store_true",
                    help="exit 1 if anything is reachable from Linux alone")
    args = ap.parse_args(argv)
    if args.self_test:
        return self_test()
    return report(strict=args.strict)


if __name__ == "__main__":
    sys.exit(main())

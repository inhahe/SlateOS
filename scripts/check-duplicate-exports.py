#!/usr/bin/env python3
"""Refuse a C symbol exported by both `posix` and `toolchain/stubs`.

Both crates are built into archives -- `libc.a` and `libstubs.a` -- that are
linked into EVERY userspace binary. When both define the same `no_mangle`
symbol, the link does not fail, because `.cargo/config.toml` passes
`--allow-multiple-definition`. The linker silently takes whichever it sees
first, and `-lstubs` is passed before rustc's own `-lc`.

WHAT THAT COST, on 2026-09-13. Three symbols were defined in both, and the
stub was the worse half of each pair:

    killpg                        stub returned -1 and set NO errno, despite a
                                  doc comment saying "errno=ENOSYS" -- so a
                                  caller's perror() printed a stale unrelated
                                  error. posix's validates the group, negates
                                  with checked_neg and delegates to kill().

    posix_spawnattr_setsigdefault stub returned 0. Success, no-op. A caller
                                  believed the spawned child would have those
                                  signals reset to default. posix's is real and
                                  already had a test.

    syscall                       stub took FOUR parameters where posix's takes
                                  seven, so three argument registers were
                                  silently dropped. Its futex arm hardcoded
                                  timeout=NULL, so a FUTEX_WAIT with a timeout
                                  waited forever.

None of that was visible. The stubs file's own header states the policy that
should have prevented it -- "As our POSIX layer grows, symbols should be moved
from here to proper implementations in the posix crate" -- and the moving
happened three times while the deleting never did.

WHY THIS READS SOURCE AND NOT THE ARCHIVES. The obvious version parses the two
`.a` files' symbol indices. It was rejected for two reasons, in order:

1. The sysroot is built by `toolchain/build-sysroot.ps1`, BY HAND. On this
   tree `libstubs.a` was ten hours older than the source when I checked, so a
   gate reading it would have graded a stale artifact and said so with
   confidence. I nearly did exactly that, twice, on the day this was written.
2. The archives carry 294 duplicate unmangled symbols, of which 291 are
   compiler-rt builtins and libm routines that BOTH crates legitimately pull
   in from `compiler_builtins`. Those would all need baselining, and a
   baseline of 291 lines is a place for a real finding to hide.

The source scan has neither problem: it cannot read a stale artifact, and it
sees only symbols someone wrote `no_mangle` on. It found exactly the three.

    python scripts/check-duplicate-exports.py
    python scripts/check-duplicate-exports.py --selftest
"""

import argparse
import os
import re
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))

NL = chr(10)

# The two crates whose archives are both linked into every userspace binary.
# No other pair in this tree has that property, which is why this gate names
# them rather than scanning everything: a duplicate between two crates that
# are never linked together is not a defect.
CRATES = ("posix/src", "toolchain/stubs/src")

BASELINE = os.path.join(ROOT, "scripts", "duplicate-exports-baseline.txt")

# `#[unsafe(no_mangle)]` or the conditional form posix uses 1,470 times,
# optionally followed by more attributes, then the exported function.
EXPORT = re.compile(
    r'#\[(?:unsafe\(no_mangle\)'
    r'|cfg_attr\(target_os\s*=\s*"none",\s*unsafe\(no_mangle\)\)'
    r'|cfg_attr\(target_os\s*=\s*"none",\s*unsafe\(export_name\s*=\s*"[^"]*"\)\))\]'
    r'\s*(?:#\[[^\]]*\]\s*)*'
    r'pub (?:unsafe )?extern "C" fn (\w+)'
)


def exported_symbols(rel):
    """{symbol: [files]} for one crate's source tree."""
    out = {}
    base = os.path.join(ROOT, rel.replace("/", os.sep))
    for dirpath, dirnames, filenames in os.walk(base):
        dirnames[:] = [d for d in dirnames if d not in ("target", ".git")]
        for fn in filenames:
            if not fn.endswith(".rs"):
                continue
            path = os.path.join(dirpath, fn)
            try:
                with open(path, encoding="utf-8", errors="replace") as fh:
                    src = fh.read()
            except OSError:
                continue
            for m in EXPORT.finditer(src):
                where = os.path.relpath(path, ROOT).replace(os.sep, "/")
                out.setdefault(m.group(1), []).append(where)
    return out


def read_baseline():
    try:
        with open(BASELINE, encoding="utf-8") as fh:
            return {
                ln.split("#", 1)[0].strip()
                for ln in fh
                if ln.split("#", 1)[0].strip()
            }
    except OSError:
        return set()


def selftest():
    bad = 0
    checks = 0

    def ck(ok, msg):
        nonlocal bad, checks
        checks += 1
        if not ok:
            print("selftest FAIL: " + msg, file=sys.stderr)
            bad += 1

    # Both spellings of the attribute must be recognised. Missing the
    # conditional one would blind this to 1,470 of posix's exports.
    plain = '#[unsafe(no_mangle)]' + NL + 'pub extern "C" fn plain_one() -> i32 { 0 }'
    ck(EXPORT.findall(plain) == ["plain_one"], "the plain attribute must match")

    cond = ('#[cfg_attr(target_os = "none", unsafe(no_mangle))]' + NL
            + 'pub extern "C" fn cond_one() -> i32 { 0 }')
    ck(EXPORT.findall(cond) == ["cond_one"],
       "the cfg_attr form -- posix's usual spelling -- must match")

    unsafe_fn = ('#[unsafe(no_mangle)]' + NL
                 + 'pub unsafe extern "C" fn unsafe_one() -> i32 { 0 }')
    ck(EXPORT.findall(unsafe_fn) == ["unsafe_one"], "an unsafe fn must match")

    stacked = ('#[cfg_attr(target_os = "none", unsafe(no_mangle))]' + NL
               + '#[allow(clippy::too_many_arguments)]' + NL
               + 'pub extern "C" fn stacked_one() -> i32 { 0 }')
    ck(EXPORT.findall(stacked) == ["stacked_one"],
       "an attribute between the export and the fn must not hide it -- "
       "posix's `syscall` is written exactly this way")

    # And the near-misses, which must NOT match.
    ck(EXPORT.findall('pub extern "C" fn not_exported() -> i32 { 0 }') == [],
       "an extern fn with no no_mangle is not an exported SYMBOL")
    ck(EXPORT.findall('// #[unsafe(no_mangle)] commented out') == [],
       "a commented-out attribute alone exports nothing")

    # The corpus must be real. A regex that stopped matching would report zero
    # duplicates and read as a clean tree -- the one outcome a gate must never
    # produce.
    posix = exported_symbols(CRATES[0])
    stubs = exported_symbols(CRATES[1])
    ck(len(posix) > 1000,
       "posix should export over a thousand C symbols, found " + str(len(posix))
       + " -- the scan has lost its subject")
    ck(len(stubs) >= 1,
       "toolchain/stubs should export at least one, found " + str(len(stubs)))
    for known in ("killpg", "syscall"):
        ck(known in posix, "posix must still export " + known)

    print("selftest: " + str(checks - bad) + "/" + str(checks) + " cases pass")
    return 1 if bad else 0


def main():
    ap = argparse.ArgumentParser(
        description=__doc__,
        formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--selftest", "--self-test", dest="selftest",
                    action="store_true")
    ap.add_argument("--list", action="store_true",
                    help="print the duplicates and exit 0")
    args = ap.parse_args()

    if args.selftest:
        return selftest()

    tables = {c: exported_symbols(c) for c in CRATES}
    for crate, table in tables.items():
        if not table:
            print("check-duplicate-exports: " + crate + " exports no C symbol "
                  "at all -- the scan has lost its subject, not found a clean "
                  "tree.", file=sys.stderr)
            return 2

    a, b = (tables[c] for c in CRATES)
    dupes = sorted(set(a) & set(b))
    baseline = read_baseline()
    new = [d for d in dupes if d not in baseline]

    print("check-duplicate-exports: " + CRATES[0] + " exports "
          + str(len(a)) + ", " + CRATES[1] + " exports " + str(len(b))
          + "; " + str(len(dupes)) + " in both, " + str(len(new))
          + " not in the baseline.")
    for d in dupes:
        mark = "NEW " if d in set(new) else "    "
        print(mark + d)
        print("        " + CRATES[0] + ": " + a[d][0])
        print("        " + CRATES[1] + ": " + b[d][0])

    if args.list:
        return 0
    if new:
        print("", file=sys.stderr)
        print("A symbol above is exported by BOTH crates, and both archives "
              "are linked into every userspace binary. This will not fail the "
              "link: `--allow-multiple-definition` is passed, so the linker "
              "takes whichever it sees first, and `-lstubs` comes before "
              "`-lc`.", file=sys.stderr)
        print("If the posix one is the real implementation, DELETE the stub -- "
              "that is the policy the stubs file's own header states. Check "
              "first that posix's covers everything the stub did: when this "
              "happened for `syscall`, posix's table was missing futex, and "
              "deleting the stub alone would have removed it.", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())

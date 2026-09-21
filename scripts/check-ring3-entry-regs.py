#!/usr/bin/env python3
"""Every transition to ring 3 must leave the syscall-argument registers DEFINED.

The kernel reaches ring 3 from six places. Five of them define the general
purpose registers -- `idt.rs` (three interrupt returns) and
`syscall/entry.rs` pop them; `fork.rs` and `thread_clone.rs` move them out of
a saved frame -- and on 2026-09-21 the sixth, `spawn.rs`'s
`userspace_entry_trampoline`, defined NONE. It pushed the five IRETQ words and
jumped, so a freshly spawned process read whatever the kernel left in
rax..r15. That included the trampoline's own argument: a kernel heap pointer,
in rdi per the SysV ABI.

Two things went wrong at once, which is why this is worth a standing gate
rather than a one-line fix and a shrug:

  * ring 3 could read a kernel heap address at its first instruction, with no
    bug of its own required -- a KASLR defeat, free;
  * a user stub that sets only the registers it needs has the REST forwarded
    as syscall arguments. `sys_process_exec_with_frame` reads arg2 as the argv
    pointer, so residue in rdx made a valid exec return InvalidAddress.

WHY THESE SIX REGISTERS. rdi, rsi, rdx, r10, r8, r9 are the syscall argument
registers. Any of them left undefined at the ring-3 boundary is both a leak
outward and a garbage argument inward on the process's first syscall. All six
correct sites define all six; the bad site defined zero. There is no site in
between, so the rule needs no threshold and no judgement.

WHAT COUNTS AS DEFINING. `pop r8` (restore from stack), `mov r8, [rcx+72]`
(restore from a saved frame), or `xor r8d, r8d` (explicit clear). The 32-bit
forms count: writing `edi` zeroes the whole of `rdi` (32-bit results are
zero-extended) and is the shorter encoding, so the idiomatic fix uses them. A
gate that only understood the 64-bit names would reject the very fix that
closes the bug it exists to catch.
"""

from __future__ import annotations

import argparse
import pathlib
import re
import sys

# The six syscall-argument registers, with every spelling that writes them.
ARG_REGS = {
    "rdi": ("rdi", "edi"),
    "rsi": ("rsi", "esi"),
    "rdx": ("rdx", "edx"),
    "r10": ("r10", "r10d"),
    "r8": ("r8", "r8d"),
    "r9": ("r9", "r9d"),
}

RING3 = re.compile(r'"(iretq|sysretq)"')
ASM_OPEN = re.compile(r"\basm!\s*\(")


def defines(block: str, canonical: str) -> bool:
    """True if `block` writes `canonical` by pop, load, or clear."""
    for spelling in ARG_REGS[canonical]:
        r = re.escape(spelling)
        # pop reg
        if re.search(r'"\s*pop\s+' + r + r'\b', block):
            return True
        # mov reg, [mem]  -- restore from a saved frame
        if re.search(r'"\s*mov\s+' + r + r'\s*,\s*\[', block):
            return True
        # xor reg, reg    -- explicit clear
        if re.search(r'"\s*xor\s+' + r + r'\s*,\s*' + r + r'\b', block):
            return True
    return False


def asm_blocks(text: str):
    """Yield (line_no, block_text) for every asm! block reaching ring 3."""
    lines = text.splitlines()
    for i, line in enumerate(lines):
        if not RING3.search(line):
            continue
        # Walk back to the enclosing asm!( -- these blocks are short and
        # never nested, so the nearest opening above is the right one.
        start = None
        for j in range(i, max(-1, i - 200), -1):
            if ASM_OPEN.search(lines[j]):
                start = j
                break
        if start is None:
            continue
        yield i + 1, "\n".join(lines[start:i + 1])


def scan(root: pathlib.Path):
    bad = []
    for path in sorted(root.rglob("*.rs")):
        try:
            text = path.read_text(encoding="utf-8", errors="replace")
        except OSError:
            continue
        if not RING3.search(text):
            continue
        for line_no, block in asm_blocks(text):
            missing = [r for r in ARG_REGS if not defines(block, r)]
            if missing:
                bad.append((path, line_no, sorted(missing)))
    return bad


GOOD = '''
    unsafe { core::arch::asm!(
        "pop rdi", "pop rsi", "pop rdx", "pop r10", "pop r8", "pop r9",
        "iretq",
        options(noreturn),
    ); }
'''

ZEROED = '''
    unsafe { core::arch::asm!(
        "push {rip}",
        "xor edi, edi", "xor esi, esi", "xor edx, edx",
        "xor r10d, r10d", "xor r8d, r8d", "xor r9d, r9d",
        "iretq",
        options(noreturn),
    ); }
'''

NONE = '''
    unsafe { core::arch::asm!(
        "push {ss}", "push {rsp_val}", "push {rflags}", "push {cs}", "push {rip}",
        "iretq",
        options(noreturn),
    ); }
'''

PARTIAL = '''
    unsafe { core::arch::asm!(
        "pop rdi", "pop rsi", "pop rdx", "pop r10", "pop r8",
        "iretq",
        options(noreturn),
    ); }
'''


def self_test() -> int:
    """Fixtures, not the tree: a self-test that reads the tree passes for the
    wrong reason the moment the tree is fixed."""
    import tempfile

    cases = [
        ("restored-by-pop", GOOD, []),
        ("zeroed-32bit", ZEROED, []),
        ("defines-nothing", NONE, ["r10", "r8", "r9", "rdi", "rdx", "rsi"]),
        ("five-of-six", PARTIAL, ["r9"]),
    ]
    failures = 0
    with tempfile.TemporaryDirectory() as td:
        for name, body, want in cases:
            d = pathlib.Path(td) / name
            d.mkdir()
            (d / "f.rs").write_text(body, encoding="utf-8", newline="")
            # Without newline=EMPTY, Windows rewrites each line ending in the
            # fixture, invisibly: git shows nothing, because a file declared
            # `text eol=lf` that is CRLF on disk matches the index anyway.
            # Caught by check-text-mode-writes on the first boot after this gate
            # was added -- a gate I wrote tripping a gate someone else wrote.
            got = scan(d)
            missing = sorted(got[0][2]) if got else []
            if missing != want:
                print("  SELF-TEST FAIL %s: expected %s, got %s"
                      % (name, want, missing))
                failures += 1
            else:
                print("  self-test ok: %s -> %s"
                      % (name, missing if missing else "clean"))
    if failures:
        print("self-test FAILED (%d case(s))" % failures)
        return 1
    # The partial case is the one that matters: it proves the gate is not
    # merely "any register mentioned" but checks each of the six.
    print("self-test passed (4 cases: restore, 32-bit zero, none, five-of-six)")
    return 0


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("root", nargs="?", default="kernel/src")
    ap.add_argument("--selftest", "--self-test", dest="selftest",
                    action="store_true")
    args = ap.parse_args()

    if args.selftest:
        return self_test()

    root = pathlib.Path(args.root)
    if not root.is_dir():
        print("not a directory: %s" % root)
        return 2
    bad = scan(root)
    if not bad:
        print("ok: every ring-3 transition defines all six syscall-argument registers")
        return 0
    print("A transition to ring 3 leaves syscall-argument registers UNDEFINED.")
    print("")
    for path, line_no, missing in bad:
        print("  %s:%d -- undefined: %s" % (path, line_no, ", ".join(missing)))
    print("")
    print("Userspace reads kernel register residue at its first instruction,")
    print("and any of these forwarded to a syscall is a garbage argument.")
    print("Define them: pop them, load them from a saved frame, or xor them.")
    return 1


if __name__ == "__main__":
    sys.exit(main())

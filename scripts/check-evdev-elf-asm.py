#!/usr/bin/env python3
"""Disassemble the hand-assembled ring-3 evdev test payload, and prove it is the one that ships.

`kernel/src/proc/elf.rs::build_linux_evdev_test_elf` emits x86-64 machine code
as byte literals.  A wrong encoding there does not fail the build -- it fails at
run time as a triple fault or a wild syscall, from a program that exists only as
a `Vec<u8>` inside the kernel, which is the worst possible place to debug it.

This script mirrors the same emission sequence and runs the result through
capstone, so the encodings can be read back as instructions before the kernel
ever executes them.  It is a developer check, not part of the build: run it
after touching the byte literals in that function.

    python scripts/check-evdev-elf-asm.py

WHAT CHANGED, AND WHY IT MATTERED
---------------------------------
The mirror below is a hand transcription of machine code, and for as long as
this file existed **nothing compared it to `elf.rs`**.  Change a byte in the
Rust and not here, and this script kept disassembling the old program and
kept printing a clean report -- a checker about code that no longer ships,
which is worse than no checker because it reads as evidence.  It is the same
defect `check-shellquote-vs-bash.py` guards against in its own port, and the
same one `check-kshell-rungs-vs-bash.py` had in its `rust_src` field.

`scripts/rustemit.py` now rebuilds the buffer *from `elf.rs` itself* -- the
byte literals, the constants (read from `evdev.rs`), the `_IOC` bit layout
(read from `evdev::ioc`), and the encodings of the `sentinel` / `jcc` /
`ioctl_call` helpers (read from their bodies) -- and the two constructions are
compared byte for byte before anything is disassembled.  Only the jump
displacements and the path's load address are exempt, because those are
patched after emission and are not knowable from source text; the instructions
carrying them are compared in full, and the displacements are checked
separately by the jump-boundary pass at the bottom.

The mirror is deliberately *not* rewritten to import the constants it now gets
checked against.  Sharing them would make the two agree by construction about
exactly the values most likely to drift.  Kept separate, a changed `KEY_MAX` or
a renumbered `EVIOC_*` now fails loudly, where before it changed nothing.

BOTH PROGRAMS ARE COVERED
-------------------------
`build_linux_evdev_test_elf` builds two: the full interrogation, and -- with
`expect_denied` -- a three-instruction program that requires the open to fail
with `EACCES`, which is what proves the `InputDevice` capability gate denies a
process that was not granted it.  Only the first was ever disassembled here.
The second is small, but "small" is not "checked", and it is the half that
fails closed.
"""

import pathlib
import sys

try:
    from capstone import Cs, CS_ARCH_X86, CS_MODE_64
except ImportError:
    # Exit 2, not 1: this gate has not found a fault, it has failed to look.
    # `run-checker.sh` and `pre-boot.py` both read 2 as "no verdict" and say so
    # rather than printing `ok`, which is the only honest thing to print about
    # a disassembly that never ran. `sys.exit("...")` -- which this used to do
    # -- exits 1, i.e. claims the payload is wrong.
    print(
        "capstone not installed, so the payload was not disassembled: "
        "pip install capstone",
        file=sys.stderr,
    )
    sys.exit(2)

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
import rustemit  # noqa: E402

REPO = pathlib.Path(__file__).resolve().parent.parent
ELF_RS = REPO / "kernel" / "src" / "proc" / "elf.rs"
EVDEV_RS = REPO / "kernel" / "src" / "evdev.rs"
FN = "build_linux_evdev_test_elf"

# Every `evdev::` constant the emission references. Named here only so that a
# rename is reported as a rename; the values come from the Rust.
EVDEV_CONSTS = (
    "IOC_READ", "IOC_WRITE", "EVDEV_IOC_MAGIC", "EV_VERSION", "BUS_I8042",
    "CLOCK_MONOTONIC", "KEY_MAX", "KEY_BYTES",
    "EVIOC_NR_GVERSION", "EVIOC_NR_GID", "EVIOC_NR_GNAME", "EVIOC_NR_GUNIQ",
    "EVIOC_NR_GKEY", "EVIOC_NR_GBIT_BASE", "EVIOC_NR_GRAB", "EVIOC_NR_SCLOCKID",
)


# ---------------------------------------------------------------------------
# The hand mirror -- an independent construction, kept for that independence.
# ---------------------------------------------------------------------------

IOC_READ, IOC_WRITE = 2, 1
MAGIC = 0x45


def ioc(direction, nr, size):
    return ((direction & 3) << 30) | ((size & 0x3FFF) << 16) | (MAGIC << 8) | (nr & 0xFF)


NR = {
    "GVERSION": 0x01, "GID": 0x02, "GNAME": 0x06, "GUNIQ": 0x08,
    "GKEY": 0x18, "GBIT": 0x20, "GRAB": 0x90, "SCLOCKID": 0xA0,
}
EV_VERSION = 0x0001_0001
BUS_I8042 = 0x11
KEY_BYTES = 96
CLOCK_MONOTONIC = 1

JS, JNZ, JZ, JLE = 0x88, 0x85, 0x84, 0x8E
RDX_RSP = (0x48, 0x8D, 0x14, 0x24)
RDX_RSP08 = (0x48, 0x8D, 0x54, 0x24, 0x08)
RDX_RSP10 = (0x48, 0x8D, 0x54, 0x24, 0x10)


def build_mirror(expect_denied):
    """Transcribe the Rust by hand. Returns (code, fail, read_ok)."""
    code = bytearray()
    fail_sites = []

    def emit(*b):
        code.extend(bytes(b))

    def sentinel(v):
        emit(0xBF, *v.to_bytes(4, "little"))

    def jcc(cc, sites):
        emit(0x0F, cc, 0, 0, 0, 0)
        sites.append(len(code) - 4)

    def ioctl_call(request, set_rdx):
        emit(0x4C, 0x89, 0xC7)
        emit(0xBE, *request.to_bytes(4, "little"))
        emit(*set_rdx)
        emit(0xB8, 0x10, 0x00, 0x00, 0x00, 0x0F, 0x05)

    read_ok = None

    emit(0x48, 0x81, 0xEC, 0x80, 0x00, 0x00, 0x00)          # sub rsp, 0x80
    emit(0x48, 0xBF, *([0] * 8))                             # movabs rdi, &path
    emit(0xBE, 0x00, 0x08, 0x00, 0x00)                       # mov esi, O_RDONLY|O_NONBLOCK
    emit(0x31, 0xD2)                                         # xor edx, edx
    emit(0xB8, 0x02, 0x00, 0x00, 0x00, 0x0F, 0x05)           # mov eax, 2; syscall

    if expect_denied:
        # Without the InputDevice capability the open must fail with EACCES.
        # Any other answer -- a success above all -- means the gate is absent.
        sentinel(0x21)
        emit(0x48, 0x83, 0xF8, 0xF3)                         # cmp rax, -EACCES
        jcc(JNZ, fail_sites)
    else:
        sentinel(0xE1)
        emit(0x48, 0x85, 0xC0)
        jcc(JS, fail_sites)
        emit(0x49, 0x89, 0xC0)                               # mov r8, rax

        emit(0xC7, 0x04, 0x24, 0, 0, 0, 0)                   # mov dword [rsp], 0
        ioctl_call(ioc(IOC_READ, NR["GVERSION"], 4), RDX_RSP)
        sentinel(0xE2)
        emit(0x48, 0x85, 0xC0)
        jcc(JNZ, fail_sites)
        sentinel(0xE3)
        emit(0x8B, 0x04, 0x24)                               # mov eax, [rsp]
        emit(0x3D, *EV_VERSION.to_bytes(4, "little"))        # cmp eax, EV_VERSION
        jcc(JNZ, fail_sites)

        emit(0x48, 0xC7, 0x04, 0x24, 0, 0, 0, 0)             # mov qword [rsp], 0
        ioctl_call(ioc(IOC_READ, NR["GID"], 8), RDX_RSP)
        sentinel(0xE4)
        emit(0x48, 0x85, 0xC0)
        jcc(JNZ, fail_sites)
        sentinel(0xE5)
        emit(0x0F, 0xB7, 0x04, 0x24)                         # movzx eax, word [rsp]
        emit(0x83, 0xF8, BUS_I8042)                          # cmp eax, 0x11
        jcc(JNZ, fail_sites)

        emit(0xC6, 0x44, 0x24, 0x10, 0x00)                   # mov byte [rsp+0x10], 0
        ioctl_call(ioc(IOC_READ, NR["GNAME"], 64), RDX_RSP10)
        sentinel(0xE6)
        emit(0x48, 0x85, 0xC0)
        jcc(JLE, fail_sites)
        sentinel(0xE7)
        emit(0x80, 0x7C, 0x24, 0x10, ord("A"))               # cmp byte [rsp+0x10], 'A'
        jcc(JNZ, fail_sites)

        emit(0xC7, 0x44, 0x24, 0x10, 0, 0, 0, 0)             # mov dword [rsp+0x10], 0
        ioctl_call(ioc(IOC_READ, NR["GBIT"], 4), RDX_RSP10)
        sentinel(0xE8)
        emit(0x48, 0x83, 0xF8, 0x04)                         # cmp rax, 4
        jcc(JNZ, fail_sites)
        sentinel(0xE9)
        emit(0xF6, 0x44, 0x24, 0x10, 0x02)                   # test byte [rsp+0x10], 2
        jcc(JZ, fail_sites)

        ioctl_call(ioc(IOC_READ, NR["GKEY"], KEY_BYTES), RDX_RSP10)
        sentinel(0xEA)
        emit(0x48, 0x83, 0xF8, KEY_BYTES)                    # cmp rax, 96
        jcc(JNZ, fail_sites)

        ioctl_call(ioc(IOC_READ, NR["GUNIQ"], 64), RDX_RSP10)
        sentinel(0xEB)
        emit(0x48, 0x83, 0xF8, 0xFE)                         # cmp rax, -2
        jcc(JNZ, fail_sites)

        emit(0x4C, 0x89, 0xC7)
        emit(0x48, 0x8D, 0x74, 0x24, 0x10)                   # lea rsi, [rsp+0x10]
        emit(0xBA, 0x18, 0x00, 0x00, 0x00)                   # mov edx, 24
        emit(0x31, 0xC0, 0x0F, 0x05)                         # xor eax, eax; syscall
        sentinel(0xEC)
        emit(0x48, 0x83, 0xF8, 0xF5)                         # cmp rax, -11
        read_ok_sites = []
        jcc(JZ, read_ok_sites)
        emit(0x48, 0x85, 0xC0)
        jcc(JLE, fail_sites)
        read_ok = len(code)
        for s in read_ok_sites:
            code[s:s + 4] = (read_ok - (s + 4)).to_bytes(4, "little", signed=True)

        emit(0x4C, 0x89, 0xC7)
        emit(0x48, 0x8D, 0x74, 0x24, 0x10)
        emit(0xBA, 0x08, 0x00, 0x00, 0x00)                   # mov edx, 8
        emit(0x31, 0xC0, 0x0F, 0x05)
        sentinel(0xED)
        emit(0x48, 0x83, 0xF8, 0xEA)                         # cmp rax, -22
        jcc(JNZ, fail_sites)

        grab = ioc(IOC_WRITE, NR["GRAB"], 4)
        ioctl_call(grab, (0xBA, 0x01, 0x00, 0x00, 0x00))     # mov edx, 1
        sentinel(0xEE)
        emit(0x48, 0x85, 0xC0)
        jcc(JNZ, fail_sites)
        ioctl_call(grab, (0x31, 0xD2))                       # xor edx, edx
        sentinel(0xEF)
        emit(0x48, 0x85, 0xC0)
        jcc(JNZ, fail_sites)

        emit(0xC7, 0x44, 0x24, 0x08, *CLOCK_MONOTONIC.to_bytes(4, "little"))
        ioctl_call(ioc(IOC_WRITE, NR["SCLOCKID"], 4), RDX_RSP08)
        sentinel(0xF0)
        emit(0x48, 0x85, 0xC0)
        jcc(JNZ, fail_sites)

        ioctl_call(0x1234, (0x31, 0xD2))
        sentinel(0xF1)
        emit(0x48, 0x83, 0xF8, 0xE7)                         # cmp rax, -25
        jcc(JNZ, fail_sites)

        emit(0x4C, 0x89, 0xC7)
        emit(0xB8, 0x03, 0x00, 0x00, 0x00, 0x0F, 0x05)       # mov eax, 3; syscall
        sentinel(0xF2)
        emit(0x48, 0x85, 0xC0)
        jcc(JNZ, fail_sites)

    emit(0x31, 0xFF)
    emit(0xB8, 0x3C, 0x00, 0x00, 0x00, 0x0F, 0x05)           # exit(0)
    fail = len(code)
    emit(0xB8, 0x3C, 0x00, 0x00, 0x00, 0x0F, 0x05)           # fail: exit(edi)
    emit(0xCC)
    for s in fail_sites:
        code[s:s + 4] = (fail - (s + 4)).to_bytes(4, "little", signed=True)

    return bytes(code), fail, read_ok


# ---------------------------------------------------------------------------
# The same buffer, rebuilt from the Rust.
# ---------------------------------------------------------------------------


def _brace_match(text, open_idx):
    depth = 0
    for i in range(open_idx, len(text)):
        if text[i] == "{":
            depth += 1
        elif text[i] == "}":
            depth -= 1
            if depth == 0:
                return i
    raise rustemit.RustEmitError(f"unbalanced braces from offset {open_idx}")


def _function_text(source, name):
    """The whole body of `fn name`, comments already stripped."""
    src = rustemit.strip_comments(source)
    marker = f"fn {name}("
    idx = src.find(marker)
    if idx < 0:
        raise rustemit.RustEmitError(
            f"`fn {name}` not found in {ELF_RS.name}.\n"
            "  It was renamed, moved, or this script is pointed at the wrong "
            "file.\n  Nothing below could be about the program that ships."
        )
    brace = src.index("{", src.index(")", idx))
    return src[idx:brace], src[brace + 1:_brace_match(src, brace)]


def rebuild_from_rust(expect_denied):
    """Replay `elf.rs`'s emission. Returns (code, wildcards)."""
    elf_src = ELF_RS.read_text(encoding="utf-8")
    evdev_src = EVDEV_RS.read_text(encoding="utf-8")

    consts = rustemit.load_consts(evdev_src, EVDEV_CONSTS)
    _sig, body = _function_text(elf_src, FN)

    anchor = "let mut code: Vec<u8> = Vec::new();"
    if anchor not in body:
        raise rustemit.RustEmitError(
            f"`{anchor}` not found in {FN}.\n"
            "  That line is where the emission starts; without it this script\n"
            "  cannot tell the helper definitions from the program they build."
        )
    split = body.index(anchor)
    header, emission = body[:split], body[split:]

    marker = "if expect_denied {"
    if marker not in emission:
        raise rustemit.RustEmitError(
            f"`{marker}` not found -- {FN} no longer builds two programs, or "
            "builds\n  them differently. The capability-denial half would go "
            "unchecked."
        )
    i = emission.index(marker)
    open_a = i + len(marker) - 1
    close_a = _brace_match(emission, open_a)
    denied = emission[open_a + 1:close_a]
    rest = emission[close_a + 1:]
    open_b = rest.index("{")
    if rest[:open_b].strip() != "else":
        raise rustemit.RustEmitError(
            "the `if expect_denied` block is not followed by a plain `else`; "
            "the two\n  programs can no longer be separated by this reader."
        )
    close_b = _brace_match(rest, open_b)
    other = rest[open_b + 1:close_b]
    tail = rest[close_b + 1:]
    tail = tail[:tail.index("let path: &[u8]")]

    em = rustemit.Emitter(
        consts,
        value_fns={
            **rustemit.load_value_fns(evdev_src, ("ioc",)),
            **rustemit.load_value_fns(header, ("gread",)),
        },
    )
    em.learn_helpers(header, ("sentinel", "jcc", "ioctl_call"))
    em.learn_local_consts(header)
    em.run(emission[:i] + (denied if expect_denied else other) + tail)
    em.mark_patched(body)
    return bytes(em.code), em.wildcards


# ---------------------------------------------------------------------------


def disassemble(code, label):
    """Print the program and check it decodes cleanly. Returns a failure count."""
    md = Cs(CS_ARCH_X86, CS_MODE_64)
    instructions = list(md.disasm(bytes(code), 0))
    starts = {ins.address for ins in instructions}
    bad = 0

    print(f"\n=== {label} ===")
    for ins in instructions:
        print(f"{ins.address:04x}  {ins.bytes.hex():<20} {ins.mnemonic} {ins.op_str}")

    end = instructions[-1].address + instructions[-1].size if instructions else 0
    print(f"\n{len(code)} bytes, {len(instructions)} instructions, "
          f"decoded through 0x{end:x}")
    if end != len(code):
        print("MISMATCH: disassembly did not cover the whole buffer")
        bad += 1

    # Every jump target must land on an instruction boundary, or a mispatched
    # displacement would decode as garbage at run time. This is also what checks
    # the displacements themselves, which the byte comparison exempts.
    for ins in instructions:
        if ins.mnemonic.startswith("j") and ins.op_str.startswith("0x"):
            tgt = int(ins.op_str, 16)
            if tgt not in starts:
                print(f"MISMATCH: jump at 0x{ins.address:x} targets 0x{tgt:x}, "
                      "not a boundary")
                bad += 1
    return bad


def main():
    bad = 0
    for expect_denied in (False, True):
        label = "expect_denied=true (capability gate)" if expect_denied else \
                "expect_denied=false (full interrogation)"
        mirror, fail, read_ok = build_mirror(expect_denied)

        # Before a single instruction is printed: the bytes about to be read
        # back must be the bytes elf.rs emits. A disassembly of the wrong
        # program is not a weaker check than none, it is a misleading one.
        built, wildcards = rebuild_from_rust(expect_denied)
        diffs = rustemit.compare(built, mirror, wildcards)
        if diffs:
            print(f"\n=== {label} ===")
            print("MIRROR HAS DRIFTED FROM elf.rs -- the disassembly below would "
                  "be of a\nprogram that is not the one the kernel builds.")
            for d in diffs:
                print(f"  {d}")
            bad += 1
            continue
        print(f"[{label}] mirror matches elf.rs: {len(built)} bytes, "
              f"{len(wildcards)} patched byte(s) exempt")

        bad += disassemble(mirror, label)
        print(f"fail label at 0x{fail:x}" +
              (f", read_ok at 0x{read_ok:x}" if read_ok is not None else ""))
    return bad


if __name__ == "__main__":
    sys.exit(main())

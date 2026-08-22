#!/usr/bin/env python3
"""Disassemble the hand-assembled ring-3 evdev test payload.

`kernel/src/proc/elf.rs::build_linux_evdev_test_elf` emits x86-64 machine code
as byte literals.  A wrong encoding there does not fail the build -- it fails at
run time as a triple fault or a wild syscall, from a program that exists only as
a `Vec<u8>` inside the kernel, which is the worst possible place to debug it.

This script mirrors the same emission sequence and runs the result through
capstone, so the encodings can be read back as instructions before the kernel
ever executes them.  It is a developer check, not part of the build: run it
after touching the byte literals in that function.

    python scripts/check-evdev-elf-asm.py
"""

import sys

try:
    from capstone import Cs, CS_ARCH_X86, CS_MODE_64
except ImportError:
    sys.exit("capstone not installed: pip install capstone")

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


JS, JNZ, JZ, JLE = 0x88, 0x85, 0x84, 0x8E
RDX_RSP = (0x48, 0x8D, 0x14, 0x24)
RDX_RSP08 = (0x48, 0x8D, 0x54, 0x24, 0x08)
RDX_RSP10 = (0x48, 0x8D, 0x54, 0x24, 0x10)

emit(0x48, 0x81, 0xEC, 0x80, 0x00, 0x00, 0x00)          # sub rsp, 0x80
emit(0x48, 0xBF, *([0] * 8))                             # movabs rdi, &path
emit(0xBE, 0x00, 0x08, 0x00, 0x00)                       # mov esi, O_RDONLY|O_NONBLOCK
emit(0x31, 0xD2)                                         # xor edx, edx
emit(0xB8, 0x02, 0x00, 0x00, 0x00, 0x0F, 0x05)           # mov eax, 2; syscall

sentinel(0xE1)
emit(0x48, 0x85, 0xC0)
jcc(JS, fail_sites)
emit(0x49, 0x89, 0xC0)                                   # mov r8, rax

emit(0xC7, 0x04, 0x24, 0, 0, 0, 0)                       # mov dword [rsp], 0
ioctl_call(ioc(IOC_READ, NR["GVERSION"], 4), RDX_RSP)
sentinel(0xE2)
emit(0x48, 0x85, 0xC0)
jcc(JNZ, fail_sites)
sentinel(0xE3)
emit(0x8B, 0x04, 0x24)                                   # mov eax, [rsp]
emit(0x3D, *EV_VERSION.to_bytes(4, "little"))            # cmp eax, EV_VERSION
jcc(JNZ, fail_sites)

emit(0x48, 0xC7, 0x04, 0x24, 0, 0, 0, 0)                 # mov qword [rsp], 0
ioctl_call(ioc(IOC_READ, NR["GID"], 8), RDX_RSP)
sentinel(0xE4)
emit(0x48, 0x85, 0xC0)
jcc(JNZ, fail_sites)
sentinel(0xE5)
emit(0x0F, 0xB7, 0x04, 0x24)                             # movzx eax, word [rsp]
emit(0x83, 0xF8, BUS_I8042)                              # cmp eax, 0x11
jcc(JNZ, fail_sites)

emit(0xC6, 0x44, 0x24, 0x10, 0x00)                       # mov byte [rsp+0x10], 0
ioctl_call(ioc(IOC_READ, NR["GNAME"], 64), RDX_RSP10)
sentinel(0xE6)
emit(0x48, 0x85, 0xC0)
jcc(JLE, fail_sites)
sentinel(0xE7)
emit(0x80, 0x7C, 0x24, 0x10, ord("A"))                   # cmp byte [rsp+0x10], 'A'
jcc(JNZ, fail_sites)

emit(0xC7, 0x44, 0x24, 0x10, 0, 0, 0, 0)                 # mov dword [rsp+0x10], 0
ioctl_call(ioc(IOC_READ, NR["GBIT"], 4), RDX_RSP10)
sentinel(0xE8)
emit(0x48, 0x83, 0xF8, 0x04)                             # cmp rax, 4
jcc(JNZ, fail_sites)
sentinel(0xE9)
emit(0xF6, 0x44, 0x24, 0x10, 0x02)                       # test byte [rsp+0x10], 2
jcc(JZ, fail_sites)

ioctl_call(ioc(IOC_READ, NR["GKEY"], KEY_BYTES), RDX_RSP10)
sentinel(0xEA)
emit(0x48, 0x83, 0xF8, KEY_BYTES)                        # cmp rax, 96
jcc(JNZ, fail_sites)

ioctl_call(ioc(IOC_READ, NR["GUNIQ"], 64), RDX_RSP10)
sentinel(0xEB)
emit(0x48, 0x83, 0xF8, 0xFE)                             # cmp rax, -2
jcc(JNZ, fail_sites)

emit(0x4C, 0x89, 0xC7)
emit(0x48, 0x8D, 0x74, 0x24, 0x10)                       # lea rsi, [rsp+0x10]
emit(0xBA, 0x18, 0x00, 0x00, 0x00)                       # mov edx, 24
emit(0x31, 0xC0, 0x0F, 0x05)                             # xor eax, eax; syscall
sentinel(0xEC)
emit(0x48, 0x83, 0xF8, 0xF5)                             # cmp rax, -11
read_ok_sites = []
jcc(JZ, read_ok_sites)
emit(0x48, 0x85, 0xC0)
jcc(JLE, fail_sites)
read_ok = len(code)
for s in read_ok_sites:
    code[s:s + 4] = (read_ok - (s + 4)).to_bytes(4, "little", signed=True)

emit(0x4C, 0x89, 0xC7)
emit(0x48, 0x8D, 0x74, 0x24, 0x10)
emit(0xBA, 0x08, 0x00, 0x00, 0x00)                       # mov edx, 8
emit(0x31, 0xC0, 0x0F, 0x05)
sentinel(0xED)
emit(0x48, 0x83, 0xF8, 0xEA)                             # cmp rax, -22
jcc(JNZ, fail_sites)

grab = ioc(IOC_WRITE, NR["GRAB"], 4)
ioctl_call(grab, (0xBA, 0x01, 0x00, 0x00, 0x00))         # mov edx, 1
sentinel(0xEE)
emit(0x48, 0x85, 0xC0)
jcc(JNZ, fail_sites)
ioctl_call(grab, (0x31, 0xD2))                           # xor edx, edx
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
emit(0x48, 0x83, 0xF8, 0xE7)                             # cmp rax, -25
jcc(JNZ, fail_sites)

emit(0x4C, 0x89, 0xC7)
emit(0xB8, 0x03, 0x00, 0x00, 0x00, 0x0F, 0x05)           # mov eax, 3; syscall
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

md = Cs(CS_ARCH_X86, CS_MODE_64)
starts = set()
bad = 0
for ins in md.disasm(bytes(code), 0):
    starts.add(ins.address)
    print(f"{ins.address:04x}  {ins.bytes.hex():<20} {ins.mnemonic} {ins.op_str}")

covered = sum(1 for _ in md.disasm(bytes(code), 0))
last = list(md.disasm(bytes(code), 0))
end = last[-1].address + last[-1].size if last else 0
print(f"\n{len(code)} bytes, {covered} instructions, decoded through 0x{end:x}")
if end != len(code):
    print("MISMATCH: disassembly did not cover the whole buffer")
    bad = 1

# Every jump target must land on an instruction boundary, or a mispatched
# displacement would decode as garbage at run time.
for ins in last:
    if ins.mnemonic.startswith("j") and ins.op_str.startswith("0x"):
        tgt = int(ins.op_str, 16)
        if tgt not in starts:
            print(f"MISMATCH: jump at 0x{ins.address:x} targets 0x{tgt:x}, not a boundary")
            bad = 1
print("fail label at 0x%x, read_ok at 0x%x" % (fail, read_ok))
sys.exit(bad)

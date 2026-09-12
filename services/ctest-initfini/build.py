#!/usr/bin/env python3
"""Reproducible build recipe for the `ctest-initfini` SlateOS fixture.

Produces `ctest-initfini.elf`, a native SlateOS (`x86_64-slateos`) binary
compiled from `main.c` by `zig cc` (clang + musl headers) and linked against
the posix `libc.a` sysroot with rust-lld.  `scripts/create-ext4-rootfs.sh`
stages it at `/tests/ctest-initfini.elf`, and the kernel runs it as a ring-3
self-test that asserts the exit code (42 == every array walked in order).

It is the first program in the tree whose `__init_array_start` and friends are
non-null, which is what `known-issues.md` -> D-CRT-INIT-ARRAY has been waiting
for since 2026-07-01.  See `main.c`'s header for what the exit codes mean and
why the fixture is C rather than C++.

## Why this recipe checks the ELF and the other nine do not

Because the thing being validated is a property of the *linked file*, and it
can be absent while everything else looks right.  Lane A measured that a C++
translation unit emits `.init_array` and no `.fini_array` at all
(`requests/a-b-crt-init-array-consumer-must-be-c-not-cpp.md`).  If this
fixture's source were ever changed in a way that lost a section -- switched to
C++, an attribute dropped, `--gc-sections` added to the link -- the binary
would still build, still run, and still exercise half the mechanism, while an
entry naming both halves read as closed.

So the build refuses unless all three arrays are present and non-empty, and
unless the six boundary symbols the crt walks are *defined* rather than left
null by a weak reference nothing satisfied.

## And why it then builds the fixture a second time, deliberately broken

A check that has never been observed to fail is a check nobody has tested.  The
second compile defines `CTEST_INITFINI_NO_FINI`, which reproduces the C++ shape
in C: the destructor functions are still emitted and still in the symbol table,
and no `.fini_array` section exists.  The build requires the same check to
reject that binary, then deletes it.  The negative control is linked into a
temporary directory rather than beside the fixture, because
`create-ext4-rootfs.sh` stages every `services/ctest-*/*.elf` it can find and a
fixture nothing runs is worse than no fixture.

Run with fastpy on PYTHONPATH so `compiler` is importable, from the root of
the worktree you are actually working in.  There are four checkouts of this
repo, and naming one of them in a command is how a lane ends up building
another lane's artifact -- see `scripts/lib/worktree.sh`:

    PYTHONPATH="D:/visual studio projects/fastpy" \
        python services/ctest-initfini/build.py

The posix sysroot (`libc.a`) must already be built and must be *current* with
`posix/src/` and with `toolchain/build-sysroot.ps1`'s RUSTFLAGS.
"""

import shutil
import struct
import subprocess
import sys
import tempfile
from pathlib import Path

from compiler import toolchain

HERE = Path(__file__).resolve().parent
OS_ROOT = HERE.parent.parent
SYSROOT_LIB = OS_ROOT / "toolchain" / "sysroot" / "lib"

# Every array the crt walks, and how many entries this fixture puts in each.
# `main.c` places two constructors and two destructors on purpose: one entry
# cannot tell "walked the array" from "ran the single thing it found".
REQUIRED_ARRAYS = {
    ".preinit_array": 1,
    ".init_array": 2,
    ".fini_array": 2,
}

# The weak externs in `posix/src/crt.rs`.  A weak *undefined* symbol resolves to
# address 0 rather than failing the link, which is what lets pure-Rust programs
# link -- and is also what would make this fixture a silent no-op if lld stopped
# synthesising the bounds.  The walk cannot tell those two apart at run time.
REQUIRED_SYMBOLS = (
    "__preinit_array_start", "__preinit_array_end",
    "__init_array_start", "__init_array_end",
    "__fini_array_start", "__fini_array_end",
)

SHN_UNDEF = 0


def _sections(data: bytes) -> dict[str, tuple[int, int, int]]:
    """Map section name -> (offset, size, entsize) for an ELF64 little-endian file."""
    if len(data) < 64 or data[:4] != b"\x7fELF" or data[4] != 2:
        raise RuntimeError("not a 64-bit ELF file")
    e_shoff, = struct.unpack_from("<Q", data, 0x28)
    e_shentsize, e_shnum, e_shstrndx = struct.unpack_from("<HHH", data, 0x3A)
    if e_shoff == 0 or e_shnum == 0:
        raise RuntimeError("ELF has no section headers")

    def header(i: int) -> tuple[int, int, int, int, int]:
        base = e_shoff + i * e_shentsize
        sh_name, = struct.unpack_from("<I", data, base)
        sh_offset, sh_size = struct.unpack_from("<QQ", data, base + 0x18)
        sh_link, = struct.unpack_from("<I", data, base + 0x28)
        sh_entsize, = struct.unpack_from("<Q", data, base + 0x38)
        return sh_name, sh_offset, sh_size, sh_link, sh_entsize

    _, str_off, str_size, _, _ = header(e_shstrndx)
    strtab = data[str_off:str_off + str_size]

    def name_at(off: int) -> str:
        end = strtab.find(b"\0", off)
        return strtab[off:end if end >= 0 else len(strtab)].decode("utf-8", "replace")

    out = {}
    for i in range(e_shnum):
        sh_name, sh_offset, sh_size, _sh_link, sh_entsize = header(i)
        out[name_at(sh_name)] = (sh_offset, sh_size, sh_entsize)
    return out


def _defined_symbols(data: bytes, sections: dict[str, tuple[int, int, int]]) -> set[str]:
    """Names in `.symtab` whose `st_shndx` is not SHN_UNDEF."""
    if ".symtab" not in sections or ".strtab" not in sections:
        return set()
    sym_off, sym_size, _ = sections[".symtab"]
    str_off, str_size, _ = sections[".strtab"]
    strtab = data[str_off:str_off + str_size]
    defined = set()
    for off in range(sym_off, sym_off + sym_size - 23, 24):
        st_name, = struct.unpack_from("<I", data, off)
        st_shndx, = struct.unpack_from("<H", data, off + 6)
        if st_shndx == SHN_UNDEF:
            continue
        end = strtab.find(b"\0", st_name)
        defined.add(strtab[st_name:end if end >= 0 else len(strtab)].decode("utf-8", "replace"))
    return defined


def check_elf(path: Path) -> list[str]:
    """Every reason `path` is not a usable init/fini consumer.  Empty == usable."""
    data = path.read_bytes()
    sections = _sections(data)
    problems = []

    for name, want_entries in REQUIRED_ARRAYS.items():
        if name not in sections:
            problems.append(f"{name}: section absent entirely")
            continue
        _off, size, entsize = sections[name]
        stride = entsize or 8
        entries = size // stride if stride else 0
        if entries < want_entries:
            problems.append(f"{name}: {entries} entr(ies), expected at least {want_entries}")

    defined = _defined_symbols(data, sections)
    if defined:
        missing = [s for s in REQUIRED_SYMBOLS if s not in defined]
        if missing:
            problems.append(
                "boundary symbols left undefined (the crt's weak externs would "
                "read as null and the walk would be a silent no-op): "
                + ", ".join(missing)
            )
    else:
        problems.append(".symtab/.strtab absent: cannot confirm the boundary symbols")

    return problems


def compile_obj(zig: str, obj: Path, defines: list[str]) -> None:
    cmd = [
        str(zig), "cc",
        f"--target={toolchain._SLATEOS_ZIG_TARGET}",
        "-c", "-O2",
        "-mcmodel=large",         # match codegen code-model=large
        "-fno-pic", "-fno-pie",   # match relocation-model=static
        "-Wall", "-Wextra", "-Werror",
        *defines,
        str(HERE / "main.c"),
        "-o", str(obj),
    ]
    result = subprocess.run(cmd, capture_output=True, text=True, timeout=300)
    if result.returncode != 0 or not obj.exists():
        sys.exit(f"C cross-compile failed:\n{result.stdout}\n{result.stderr}")


def main() -> None:
    zig = toolchain._find_zig_cc()
    if zig is None:
        sys.exit(
            "Cannot find `zig` for the SlateOS C cross-compile. Install zig "
            "(it bundles clang + musl), put it on PATH, or set FASTPY_ZIG."
        )
    if not (SYSROOT_LIB / "libc.a").exists():
        sys.exit(f"Missing sysroot libc.a in {SYSROOT_LIB}; run toolchain/build-sysroot.ps1")

    # --- the negative control, first, so a broken check cannot pass a broken
    # fixture on the way past ---------------------------------------------------
    scratch = Path(tempfile.mkdtemp(prefix="ctest-initfini-"))
    try:
        bad_obj = scratch / "nofini.o"
        compile_obj(zig, bad_obj, ["-DCTEST_INITFINI_NO_FINI"])
        bad_exe = toolchain._link_slateos(
            [bad_obj],
            scratch / "nofini.out",
            entry="_start",
            sysroot_lib_dir=SYSROOT_LIB,
            libs=["c"],
        )
        refused = check_elf(Path(bad_exe))
        if not any(p.startswith(".fini_array") for p in refused):
            sys.exit(
                "NEGATIVE CONTROL PASSED THE CHECK. A binary built without any\n"
                "`.fini_array` was accepted, so the check below proves nothing\n"
                "about the real fixture. Reasons it did give: "
                + ("; ".join(refused) or "(none)")
            )
        print("REFUSED (as required):", "; ".join(refused))
    finally:
        shutil.rmtree(scratch, ignore_errors=True)

    # --- the fixture ------------------------------------------------------------
    obj = HERE / "main.o"
    compile_obj(zig, obj, [])
    exe = toolchain._link_slateos(
        [obj],
        HERE / "ctest-initfini.elf",
        entry="_start",
        sysroot_lib_dir=SYSROOT_LIB,
        libs=["c"],
    )
    problems = check_elf(Path(exe))
    if problems:
        Path(exe).unlink(missing_ok=True)
        sys.exit(
            "The linked fixture is not a usable init/fini consumer, so it has\n"
            "been deleted rather than staged:\n  " + "\n  ".join(problems)
        )

    print("OBJ:", obj, obj.stat().st_size)
    print("EXE:", exe, Path(exe).stat().st_size)


if __name__ == "__main__":
    main()

#!/usr/bin/env python3
"""Reproducible build recipe for the `ctest-fortify` SlateOS fixture.

Produces `ctest-fortify.elf`, a native SlateOS (`x86_64-slateos`) binary
compiled from `main.c` by `zig cc` (clang + musl headers) and linked against
the posix `libc.a` sysroot with rust-lld.  `scripts/create-ext4-rootfs.sh`
stages it at `/tests/ctest-fortify.elf`, and the kernel runs it as a ring-3
self-test (`self_test_cfortify`, `kernel/src/proc/spawn.rs`) that asserts the
exit code (42 == all checks passed).

It covers the `_FORTIFY_SOURCE` printf family — `__printf_chk`,
`__fprintf_chk`, `__dprintf_chk`, `__asprintf_chk`, `__sprintf_chk`,
`__snprintf_chk` — whose entry points are assembly trampolines
(`va_trampoline!` in `posix/src/fortify_printf.rs`) that exist only on the
bare-metal target.  `cargo test` can reach the `__v*_chk` Rust functions but
never the trampolines, so without this fixture the twelve hard-coded
`gp_offset`/register pairs are unverified, and getting one wrong is silent:
the varargs are read one slot off and a plausible wrong number is printed.

Sibling fixtures in the sysroot float-ABI family:

    ctest-libc-float   double *returns* and *varargs* doubles
    ctest-libm         *named* float arguments (%xmm0-7 by SSE class)
    ctest-longdouble   `long double`: X87/X87UP -> MEMORY, returned in %st(0)
    ctest-fortify      the same rules again, through the `__*_chk` ABI

`main.c` declares the glibc `__*_chk` prototypes by hand: musl (whose headers
zig cc uses) deliberately implements no `_FORTIFY_SOURCE`, so
`-D_FORTIFY_SOURCE=2` would rewrite nothing.  Calling them directly is also
the more precise test — it pins the exact ABI a fortified glibc object file
expects.

`-fno-builtin` keeps clang from constant-folding `snprintf`-shaped calls with
literal arguments into precomputed strings, which would make the fixture pass
without ever entering the sysroot.  `main.c` additionally launders every input
through a volatile global for the same reason.

Otherwise the compile flags mirror `toolchain/x86_64-slateos.json` (static
relocation, large code model).

Run from the fastpy repo root so `compiler` is importable, e.g.:

    PYTHONPATH="D:/visual studio projects/fastpy" \
        python "D:/visual studio projects/os/services/ctest-fortify/build.py"

The posix sysroot (`libc.a`) must already be built and must be *current* with
`posix/src/` and with `toolchain/build-sysroot.ps1`'s RUSTFLAGS.
"""

import subprocess
import sys
from pathlib import Path

from compiler import toolchain

HERE = Path(__file__).resolve().parent
OS_ROOT = HERE.parent.parent
SYSROOT_LIB = OS_ROOT / "toolchain" / "sysroot" / "lib"


def main() -> None:
    zig = toolchain._find_zig_cc()
    if zig is None:
        sys.exit(
            "Cannot find `zig` for the SlateOS C cross-compile. Install zig "
            "(it bundles clang + musl), put it on PATH, or set FASTPY_ZIG."
        )
    if not (SYSROOT_LIB / "libc.a").exists():
        sys.exit(f"Missing sysroot libc.a in {SYSROOT_LIB}; run toolchain/build-sysroot.ps1")

    obj = HERE / "main.o"
    cmd = [
        str(zig), "cc",
        f"--target={toolchain._SLATEOS_ZIG_TARGET}",
        "-c", "-O2",
        "-fno-builtin",           # call the sysroot, don't inline/fold
        "-mcmodel=large",         # match codegen code-model=large
        "-fno-pic", "-fno-pie",   # match relocation-model=static
        "-Wall", "-Wextra", "-Werror",
        str(HERE / "main.c"),
        "-o", str(obj),
    ]
    result = subprocess.run(cmd, capture_output=True, text=True, timeout=300)
    if result.returncode != 0 or not obj.exists():
        sys.exit(f"C cross-compile failed:\n{result.stdout}\n{result.stderr}")

    exe = toolchain._link_slateos(
        [obj],
        HERE / "ctest-fortify.elf",
        entry="_start",
        sysroot_lib_dir=SYSROOT_LIB,
        libs=["c"],
    )
    print("OBJ:", obj, obj.stat().st_size)
    print("EXE:", exe, exe.stat().st_size)


if __name__ == "__main__":
    main()

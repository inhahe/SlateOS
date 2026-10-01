#!/usr/bin/env python3
"""Reproducible build recipe for the `ctest-cxx-throw` SlateOS fixture.

Produces `ctest-cxx-throw.elf`, a native SlateOS (`x86_64-slateos`) binary:
`main.cpp` compiled by `zig c++` (clang, with libc++'s and musl's headers and
`posix/include` in front of them), linked with rust-lld against zig's C++
runtime -- libc++, libc++abi, libunwind, compiler_rt -- and the posix `libc.a`
sysroot, the way every C++ port here is linked (`scripts/cmake-spike/run.sh`).
`scripts/create-ext4-rootfs.sh` stages it at `/tests/ctest-cxx-throw.elf`;
the kernel rung that runs it and asserts the exit code (42 == all checks
passed) is lane A's, requested in `requests/d-a-run-ctest-cxx-throw.md`.

## What it is for

A C++ `throw` can only be caught if libunwind finds the program's unwind
tables, and it finds them through the C library's `dl_iterate_phdr`, which
until 2026-09-29 never called back (known-issues.md ->
`D-POSIX-DL-ITERATE-PHDR-NEVER-CALLED-BACK-SO-NO-CXX-THROW-COULD-BE-CAUGHT`).
posix's host tests hold `dl_iterate_phdr` to glibc's answers over a
synthetic image; this runs the whole chain -- libc, unwinder, C++ runtime --
on the target, where the image, the thread pointer and the unwinder are real.

## The link

It passes `--eh-frame-hdr`, which fastpy's `_link_slateos` does not and a C
fixture has no need of: the flag is what makes the linker build
`.eh_frame_hdr` and the `PT_GNU_EH_FRAME` segment libunwind searches. Without
it a program links, and every `throw` in it terminates -- which is the fault
this fixture exists to catch, arrived at from the other side. (Compilers'
own drivers pass it for every dynamic link; for static ones GCC's does not,
and relies on crtbegin registering the frames instead, which nothing here
does.)

zig's runtime archives live in its cache at content-addressed paths that
change with the zig version, so they are found by asking zig: a trivial
`zig c++ -static -v` link names them, zig's own `libc.a` excepted -- ours is
the point (`scripts/lib/worktree.sh`, `slate_zig_cxx_runtime`, does the same).

`-fno-builtin` and the codegen flags mirror `toolchain/x86_64-slateos.json`
(static relocation, large code model), as in the C fixtures.

Run with fastpy on PYTHONPATH so `compiler` is importable, from the root of
the worktree you are actually working in -- see `scripts/lib/worktree.sh`:

    PYTHONPATH="D:/visual studio projects/fastpy" \\
        python services/ctest-cxx-throw/build.py

The posix sysroot (`libc.a`) must already be built and current with
`posix/src/`: this fixture tests code added the same day, so a stale
sysroot fails it for the wrong reason.
"""

import subprocess
import sys
import tempfile
from pathlib import Path

from compiler import toolchain

HERE = Path(__file__).resolve().parent
OS_ROOT = HERE.parent.parent
SYSROOT_LIB = OS_ROOT / "toolchain" / "sysroot" / "lib"
NAME = HERE.name


def cxx_runtime(zig: Path) -> list[Path]:
    """zig's C++ runtime archives for the target, as its driver names them."""
    with tempfile.TemporaryDirectory(prefix="ctest-cxx-") as t:
        d = Path(t)
        (d / "probe.cpp").write_text("int main() { return 0; }\n", encoding="utf-8")
        r = subprocess.run(
            [str(zig), "c++", f"--target={toolchain._SLATEOS_ZIG_TARGET}", "-std=c++17",
             "-static", "-v", "-o", str(d / "probe"), str(d / "probe.cpp")],
            capture_output=True, text=True, timeout=900,
        )
    if r.returncode != 0:
        sys.exit(f"zig c++ could not link a trivial program:\n{r.stdout}\n{r.stderr}")
    found = {w.strip('"') for w in (r.stdout + " " + r.stderr).split() if w.strip('"').endswith(".a")}
    libs = sorted(Path(w) for w in found if Path(w).name != "libc.a")
    names = {p.name for p in libs}
    for need in ("libc++.a", "libc++abi.a", "libunwind.a"):
        if need not in names:
            sys.exit(f"zig c++ -v named no {need}: {sorted(names)}. Without it a C++ "
                     "link reports every symbol it holds as missing, which reads as "
                     "'our libc is incomplete'.")
    return libs


def main() -> None:
    zig = toolchain._find_zig_cc()
    if zig is None:
        sys.exit(
            "Cannot find `zig` for the SlateOS C++ cross-compile. Install zig "
            "(it bundles clang, libc++ and musl), put it on PATH, or set FASTPY_ZIG."
        )
    lld = toolchain._find_rust_lld()
    if lld is None:
        sys.exit("Cannot find rust-lld (it comes with a Rust toolchain; or set FASTPY_RUST_LLD).")
    if not (SYSROOT_LIB / "libc.a").exists():
        sys.exit(f"Missing sysroot libc.a in {SYSROOT_LIB}; run toolchain/build-sysroot.ps1")

    obj = HERE / "main.o"
    cmd = [
        str(zig), "c++",
        f"--target={toolchain._SLATEOS_ZIG_TARGET}",
        "-std=c++17",
        "-c", "-O2",
        "-fno-builtin",           # call the sysroot, don't inline/fold
        "-mcmodel=large",         # match codegen code-model=large
        "-fno-pic", "-fno-pie",   # match relocation-model=static
        "-I", str(OS_ROOT / "posix" / "include"),  # the overlay: what musl's headers lack
        "-Wall", "-Wextra", "-Werror",
        str(HERE / "main.cpp"),
        "-o", str(obj),
    ]
    result = subprocess.run(cmd, capture_output=True, text=True, timeout=600)
    if result.returncode != 0 or not obj.exists():
        sys.exit(f"C++ cross-compile failed:\n{result.stdout}\n{result.stderr}")

    exe = HERE / f"{NAME}.elf"
    link = [
        str(lld), "-flavor", toolchain._SLATEOS_LLD_FLAVOR,
        "-static", "--no-dynamic-linker",
        "--eh-frame-hdr",         # PT_GNU_EH_FRAME, which libunwind searches
        "-e", "_start",
        "-o", str(exe),
        str(obj),
        *(str(p) for p in cxx_runtime(zig)),
        f"-L{SYSROOT_LIB}", "-lc",
    ]
    result = subprocess.run(link, capture_output=True, text=True, timeout=600)
    if result.returncode != 0 or not exe.exists():
        sys.exit(f"SlateOS C++ link failed:\n{result.stdout}\n{result.stderr}")
    print("OBJ:", obj, obj.stat().st_size)
    print("EXE:", exe, exe.stat().st_size)


if __name__ == "__main__":
    main()

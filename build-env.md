# Build environment: toolchains and C dependencies


Extracted from `CLAUDE.md` on 2026-09-06. It lived there, resident in every
lane's context on every turn, and is needed only when you are actually doing
the thing it describes -- which announces itself, so a pointer suffices.
`CLAUDE.md` points here.



### C/C++ Compilers (for crates with C dependencies like libz-sys, ring, libgit2-sys)

The PC the project moved to on 2026-10-20 has one Visual Studio: **VS 2026 Build Tools**, MSVC 14.51.36231, Windows SDK 10.0.26100.0. It is **not on PATH by default**. You must either run vcvarsall.bat first or set `CC`/`CC_x86_64_slateos` to the full path.

- vcvarsall.bat: `"C:\Program Files (x86)\Microsoft Visual Studio\18\BuildTools\VC\Auxiliary\Build\vcvarsall.bat"`
- cl.exe (x64 host, x64 target): `"C:\Program Files (x86)\Microsoft Visual Studio\18\BuildTools\VC\Tools\MSVC\14.51.36231\bin\Hostx64\x64\cl.exe"`

The MSVC version in that path changes with every Build Tools update, so read
it rather than trusting the number written here: `vswhere -latest -products *
-property installationPath` gives the install's folder (vswhere is
`C:\Program Files (x86)\Microsoft Visual Studio\Installer\vswhere.exe`;
without `-products *` it looks only for full Visual Studio editions and prints
nothing here), and `VC\Auxiliary\Build\Microsoft.VCToolsVersion.default.txt`
under that folder holds the MSVC version.

For cross-compiling Rust crates with C dependencies to our custom target (x86_64-slateos), set:
```
CC_x86_64_slateos=cl.exe   (after running vcvarsall.bat x64)
```
Or from bash without vcvarsall, pass the full cl.exe path and ensure the Windows SDK include/lib dirs are also set.

### fastpy (for the ring-3 fixtures)

`scripts/ctest-fixtures.py` compiles the ~70 fixture ELFs the boot test runs in
ring 3, and the `fastpy-*` ones need the fastpy checkout importable. It looks
for fastpy **beside the repo root** and honours `$FASTPY_DIR`.

Since the move to the new PC (2026-10-20) fastpy sits beside the OS repo, at
`C:/visual studio projects/fastpy`, so the sibling search finds it with
nothing set. A second checkout remains at `D:/visual studio projects/fastpy`:
its git history is newer (the history rewrite for publishing, 2026-10-18), but
on 2026-10-21 its `compiler/` and `runtime/` sources were identical to the
sibling's. If the two ever diverge, name the one you mean:

```
FASTPY_DIR="D:/visual studio projects/fastpy" python scripts/ctest-fixtures.py build
```

fastpy needs `llvmlite`, so it must run under a Python that has it. On this PC
a bare `python` starts Python 3.15, which has no packages, while the
packages, `llvmlite` among them, are installed in Python 3.14
(`py -3.14`, or `C:/Users/Computer/AppData/Local/Python/pythoncore-3.14-64/python.exe`).
Under 3.15, `ctest-fixtures.py build` refuses before building anything and
names the missing module. Until the default is 3.14 again, run the fixture
build with `py -3.14`, and put that folder first on `PATH` for a boot test.

Note that rebuilding fixtures can rebuild `libc.a` first, and a new `libc.a`
makes *every* fixture stale — so a one-fixture build can turn into a 70-fixture
one, and the rootfs image then needs repacking too
(`wsl -d Ubuntu -- bash scripts/create-ext4-rootfs.sh`).

### zig (the libc gates and the C fixtures)

`check-libc-abi.py`, `check-libc-declared.py`, `check-libc-prototypes.py` and
`check-libc-overlay.py` read musl's headers with zig 0.16.0, found through
`$FASTPY_ZIG` and then `PATH` only. Without it each prints `SKIPPED` and exits
3: `build-sysroot.ps1` goes on with a yellow warning, and the pre-push hook
lists the libc-ABI gate among the skipped ones and lets the push through. So a
missing zig is easy to miss. fastpy's own
lookup, which the fixture builds use, also tries `D:\utils\zig-*` and
`C:\utils\zig-*`.

On this PC zig is at `D:/utils/zig-x86_64-windows-0.16.0/zig.exe`, which
neither lookup finds by itself: the user-scope `FASTPY_ZIG` carried over from
the old PC names `C:/utils/...`, which this PC lacks. Until that is corrected,
set it in your shell before a sysroot build or a push:

```
export FASTPY_ZIG="D:/utils/zig-x86_64-windows-0.16.0/zig.exe"
```

### Rust toolchains

- `nightly-x86_64-pc-windows-gnu` — primary nightly, has `dlltool.exe` issue with newer crates (getrandom v0.3+). Fixed by copying `dlltool.exe` from self-contained dir to `~/.cargo/bin/`, but it may not work for all crates.
- `nightly-x86_64-pc-windows-msvc` — alternative nightly, avoids dlltool issues, requires VS build tools on PATH for host-side C compilation.
- Custom target `toolchain/x86_64-slateos.json` requires `-Zjson-target-spec` (set via `[unstable]` in `.cargo/config.toml`, NOT via env var or CLI flag).

---

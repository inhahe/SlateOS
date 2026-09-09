# Build environment: toolchains and C dependencies


Extracted from `CLAUDE.md` on 2026-09-06. It lived there, resident in every
lane's context on every turn, and is needed only when you are actually doing
the thing it describes -- which announces itself, so a pointer suffices.
`CLAUDE.md` points here.



### C/C++ Compilers (for crates with C dependencies like libz-sys, ring, libgit2-sys)

The machine has Visual Studio and Build Tools installed but they are **not on PATH by default**. You must either run vcvarsall.bat first or set `CC`/`CC_x86_64_slateos` to the full path.

**vcvarsall.bat locations:**
- VS 2022 Community: `"C:\Program Files\Microsoft Visual Studio\2022\Community\VC\Auxiliary\Build\vcvarsall.bat"`
- VS 2022 Enterprise: `"C:\Program Files\Microsoft Visual Studio\2022\Enterprise\VC\Auxiliary\Build\vcvarsall.bat"`
- VS 2026 Build Tools: `"C:\Program Files (x86)\Microsoft Visual Studio\18\BuildTools\VC\Auxiliary\Build\vcvarsall.bat"`
- VS 2022 Build Tools: `"C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvarsall.bat"`

**cl.exe locations (x64 host → x64 target):**
- VS 2022: `"C:\Program Files\Microsoft Visual Studio\2022\Community\VC\Tools\MSVC\14.44.35207\bin\Hostx64\x64\cl.exe"`
- VS 2026 BT: `"C:\Program Files (x86)\Microsoft Visual Studio\18\BuildTools\VC\Tools\MSVC\14.50.35717\bin\Hostx64\x64\cl.exe"`

For cross-compiling Rust crates with C dependencies to our custom target (x86_64-slateos), set:
```
CC_x86_64_slateos=cl.exe   (after running vcvarsall.bat x64)
```
Or from bash without vcvarsall, pass the full cl.exe path and ensure the Windows SDK include/lib dirs are also set.

### fastpy (for the ring-3 fixtures)

`scripts/ctest-fixtures.py` compiles the ~70 fixture ELFs the boot test runs in
ring 3, and the `fastpy-*` ones need the fastpy checkout importable. It looks
for fastpy **beside the repo root** and honours `$FASTPY_DIR`.

Since the 2026-09-06 migration the sibling search no longer finds it: the OS
moved to `E:` and fastpy did not, so `<repo>/../fastpy` resolves to
`E:/visual studio projects/fastpy`, which does not exist. fastpy is still at
`D:/visual studio projects/fastpy` — see the "Still on `D:`" note at the bottom
of `E:/visual studio projects/CLAUDE.md`.

So prefix fixture builds:

```
FASTPY_DIR="D:/visual studio projects/fastpy" python scripts/ctest-fixtures.py build
```

The symptom without it is an import traceback that does not mention fastpy at
all, which is why this is written down rather than left to be rediscovered.
Note that rebuilding fixtures can rebuild `libc.a` first, and a new `libc.a`
makes *every* fixture stale — so a one-fixture build can turn into a 70-fixture
one, and the rootfs image then needs repacking too
(`wsl -d Ubuntu -- bash scripts/create-ext4-rootfs.sh`).

### Rust toolchains

- `nightly-x86_64-pc-windows-gnu` — primary nightly, has `dlltool.exe` issue with newer crates (getrandom v0.3+). Fixed by copying `dlltool.exe` from self-contained dir to `~/.cargo/bin/`, but it may not work for all crates.
- `nightly-x86_64-pc-windows-msvc` — alternative nightly, avoids dlltool issues, requires VS build tools on PATH for host-side C compilation.
- Custom target `toolchain/x86_64-slateos.json` requires `-Zjson-target-spec` (set via `[unstable]` in `.cargo/config.toml`, NOT via env var or CLI flag).

---

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

`scripts/ctest-fixtures.py` compiles the fixture ELFs the boot test runs in
ring 3, and the `fastpy-*` ones need fastpy importable. This tree pins fastpy
as the git submodule `toolchain/fastpy` (2026-10-21, operator's decision), so a
checkout says which compiler built its binaries. `ctest-fixtures.py build`
checks the submodule out at the pinned commit when it is not -- a fresh
worktree, or a merge that moved the pin -- and refuses rather than discard work
in it: changes to its files, or a commit made in it that is on none of fastpy's
branches. It records which fastpy commit built each `fastpy-*` fixture
(`toolchain/sysroot/fastpy-pins`), so a fixture built with any other is stale,
and `ctest-fixtures.py image-check` -- which the boot test runs -- refuses an
image holding one. By hand:

```
git submodule update --init -- toolchain/fastpy
```

To build with another fastpy -- working on fastpy itself, whose own checkout is
`C:/visual studio projects/fastpy` -- name it; nothing else is looked at:

```
FASTPY_DIR="<path-to-fastpy>" python scripts/ctest-fixtures.py build
```

Moving the pin to a new fastpy: push the fastpy commit to
github.com/inhahe/fastpy first -- the URL `.gitmodules` names, and so the place
every other worktree's `build` fetches it from -- then
`git -C toolchain/fastpy fetch` and `git -C toolchain/fastpy checkout <sha>`,
and commit `toolchain/fastpy`. The `fastpy-*` fixtures are then stale, and
`image-check` refuses until `ctest-fixtures.py build` has rebuilt them.

Git will not remove a worktree whose submodule has been checked out ("working
trees containing submodules cannot be moved or removed"), even after
`git submodule deinit`; one that never checked it out is removed as usual.
Check that `git -C <worktree> status` shows nothing of value, then
`git worktree remove --force <worktree>`. (Each worktree keeps its own clone
of fastpy, about 5 MB.)

fastpy needs `llvmlite`, so it must run under a Python that has it: a bare
`python` here is Python 3.14 with the project's packages. A Python without
them -- a newly installed version made the default, as 3.15 briefly was on
2026-10-21 -- makes `ctest-fixtures.py build` refuse before building
anything, naming the missing module.

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
missing zig is easy to miss: a `SKIPPED` from any of the four means set
`FASTPY_ZIG`. fastpy's own lookup, which the fixture builds use, also tries
`D:\utils\zig-*` and `C:\utils\zig-*`.

On this PC the user-scope `FASTPY_ZIG` names
`C:/utils/zig-x86_64-windows-0.16.0/zig.exe`, which exists.

### Rust toolchains

- `nightly-x86_64-pc-windows-gnu` — primary nightly, has `dlltool.exe` issue with newer crates (getrandom v0.3+). Fixed by copying `dlltool.exe` from self-contained dir to `~/.cargo/bin/`, but it may not work for all crates.
- `nightly-x86_64-pc-windows-msvc` — alternative nightly, avoids dlltool issues, requires VS build tools on PATH for host-side C compilation.
- Custom target `toolchain/x86_64-slateos.json` requires `-Zjson-target-spec` (set via `[unstable]` in `.cargo/config.toml`, NOT via env var or CLI flag).

---

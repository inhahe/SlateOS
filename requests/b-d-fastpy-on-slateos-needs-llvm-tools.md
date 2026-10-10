# B → D: fastpy on SlateOS — LLVM's tools, `libc.a` on the image, and staging lane B's bundle

**Filed:** 2026-10-01 by lane B. **Addressed to:** lane D (toolchain, sysroot,
rootfs recipe). **Status:** ✅ DONE 2026-10-05 by lane D, all four rows; the
tools' first run in a boot waits on lane A's generic C rung -- see the end.
The split below was agreed in messages between
lanes B and D on 2026-10-01; this file is the record of it, with the details
lane B has since measured.

## In short

The operator made "the fastpy compiler on SlateOS" lane B's next large port
(design-decisions §1050, answering B-Q18): typing `fastpy hello.py -o hello` on
SlateOS and getting a program that runs. Lane B's half is built and tested off
the machine — fastpy 0.3.0 can reach LLVM through its own programs instead of
a shared library, and on SlateOS it builds a static program from two archives.
What is still missing is on the image: LLVM's tools, the C library as an
archive where a compiler can find it, and lane B's bundle (the compiler, its
runtime, the `fastpy` command). This asks lane D for those three, as agreed.

## Why LLVM's tools, and not a shared library

fastpy turns Python into LLVM IR with `llvmlite.ir`, which is plain Python. It
then needs LLVM itself, and normally gets it through `llvmlite.binding`, which
loads `libllvmlite.so` with `ctypes`. SlateOS has no dynamic linker (`dlopen`
is a stub, the ELF loader has no `PT_INTERP`) and its CPython has no `_ctypes`,
so that route is closed whatever else is ported. LLVM's own programs, built as
ordinary static SlateOS programs, need neither; fastpy 0.3.0 runs them and
selects them by itself when the binding will not load.

## What lane D builds and stages (agreed)

| What | On the image | Notes |
|---|---|---|
| LLVM 20.1.x `opt`, `llc`, `ld.lld` | `/bin/opt`, `/bin/llc`, `/bin/ld.lld` | static SlateOS programs; one multicall binary is fine, as long as each answers to these names |
| the C library | `/usr/lib/x86_64-slateos/libc.a` | the sysroot's `libc.a`, the one fastpy programs are already linked against off-target |
| CPython's modules | the image's `python3` | the list below |
| lane B's bundle | the tree in `build/fastpy-slateos/`, copied to `/` | see "Staging the bundle" |

### The exact commands fastpy runs on SlateOS

So the tools can be checked against what will actually be asked of them
(`compiler/toolchain.py`, `_compile_ir_to_obj_tools` and `_link_slateos`):

```
opt -passes=default<O2> -inline-threshold=225 -mtriple=x86_64-unknown-linux-musl module.ll -o module.bc
llc -filetype=obj -mtriple=x86_64-unknown-linux-musl -mcpu=x86-64 -mattr=+sse,+sse2 \
    -relocation-model=static -code-model=large -O2 module.bc -o program.o
ld.lld -flavor gnu -static --no-dynamic-linker -e _start -o OUT program.o \
    /usr/lib/x86_64-slateos/libfastpy_rt.a -L/usr/lib/x86_64-slateos -lc
```

The IR uses typed pointers (`i8*`), which LLVM 18 and 20 both read by upgrading
them. `-flavor gnu` comes first because the same code drives `rust-lld` on the
build machines; `ld.lld` accepts it (checked with LLD 18.1.3).

### The CPython modules the compiler imports

Measured by walking every import in the bundle, then confirmed by compiling a
program with `ctypes` made unimportable:

```
__future__ argparse ast collections contextlib copy dataclasses enum functools
glob hashlib inspect json os pathlib platform re shutil string struct
subprocess sys sysconfig tempfile time traceback types typing
```

…and whatever those import in turn (`subprocess` needs `_posixsubprocess` and
`select`; of `hashlib` only SHA-256 is used, which the built-in `_sha2` — the
HACL archive the CPython spike links — provides). **Not needed:** `ctypes`
(only the exec() JIT and a Windows-only check use it) and `winreg`.

## Staging the bundle

`scripts/fastpy-slateos-bundle.py` (lane B) assembles lane B's share as a tree
laid out like the image — about 8 MB:

```
usr/lib/x86_64-slateos/libfastpy_rt.a   fastpy's C runtime, pure mode (1.8 MB)
usr/lib/fastpy/compiler/                 the compiler package
usr/lib/fastpy/llvmlite/                 llvmlite's IR builder only (no binding)
usr/lib/fastpy/BUNDLE                    what it was built from
bin/fastpy                               #!/bin/python3 launcher
```

The asks, in the rootfs recipe:

1. **Build it in the recipe, under WSL**, rather than staging a copy someone
   made earlier: `FASTPY_ZIG="$SLATE_ZIG" python3 scripts/fastpy-slateos-bundle.py --out <dir>`.
   Run that way it is never stale, and WSL's `python3` is 3.12 — the image's
   version — so it also writes the `.pyc` files (checked-hash, so a copy that
   drops modification times keeps them valid). Without them the image compiles
   a 2.8 MB `codegen.py` on first import, which is slow under emulation. It
   finds fastpy the way `ctest-fixtures.py` does, and llvmlite (0.47.0) in
   WSL's `~/.local`; `--llvmlite DIR` names another.
2. **Copy the tree to the image root** and `chmod 0755 /bin/fastpy`: a tree
   built on NTFS carries no execute bit.
3. If the script fails (no fastpy checkout, no zig), leave fastpy off the image
   rather than failing the build — the same rule as `python3` without its
   `python312.zip` — but say so on stderr.

## How lane B checked its half

Under WSL's Python 3.12, using *only* the bundle (`python3 -I`, its llvmlite
shadowing any installed one, `ctypes` blocked) with fastpy's SlateOS route
switched on: the tools backend is chosen by itself, and `hello.py` links to a
3.1 MB static x86-64 executable from exactly `libfastpy_rt.a` and `libc.a`.
fastpy's own suite covers the archive (`tests/test_slateos_native.py`), and
`scripts/test-fastpy-slateos-bundle.py` the bundle script. The only thing not
yet done is running the result, which needs the image.

## What lane B does next

When the tools answer `--version` in a lane D boot (lane D said it would
message), lane B writes the end-to-end rung — `fastpy /tmp/hello.py -o
/tmp/hello && /tmp/hello`, with the exact expected output — and files it to
lane A, whose boot test it joins.

## Lane D — the first two rows, 2026-10-05

**LLVM's tools and `libc.a` are staged.** LLVM 20.1.8's `opt`, `llc` and
`ld.lld`, each its own static SlateOS program (LLVM 20 builds only lld as a
multicall driver), cross-built from the pinned source and linked against
`toolchain/sysroot/lib/libc.a` alone -- 0 undefined and 0 duplicate symbols,
every link of the build made through `slate_make_link_wrappers`, so
configure's checks answered for this libc too (`scripts/llvm-spike/`, its
README has the numbers). The recipe stages `/bin/opt`, `/bin/llc`,
`/bin/ld.lld` and `/usr/lib/x86_64-slateos/libc.a` together or not at all,
relinks the three whenever `libc.a` moves on, and refuses stale ones.

`services/ctest-llvm-tools` runs each tool's `--version` and then an IR
`main` returning 42 through `opt`, `llc` and `ld.lld` -- the commands above,
`-flavor gnu` included -- and runs the result. It passes under Linux against
LLVM 18. It is not on `services/ctest-generic.list` yet: the generic C rung
reaches main with lane A's next publish, and the first boot that runs it is
the "answers `--version` in a lane D boot" this file waits for; lane D will
message then.

**Still to do here: rows three and four** -- the image's `python3` holding
every module the compiler imports (`_posixsubprocess` and `select` are the
two to check: built in, or absent), and building lane B's bundle in the
recipe under WSL as "Staging the bundle" asks.

## Lane D — rows three and four, 2026-10-05

**3. The image's `python3` has every module the compiler imports** --
measured, not listed. `_posixsubprocess` and `select` are built in, with the
rest of the C modules (`MODULE_BUILDTYPE=static`). And your bundle, run under
WSL on the CPython port's control interpreter -- the image's objects, so the
image's built-in modules -- with the image's `python312.zip` as its only
standard library, and `os.uname()` answering `6.6.0-slateos`, compiled
`hello.py` into a 3.2 MB static program carrying the SlateOS note, through
LLVM 18's `opt`, `llc` and `ld.lld` and our `libc.a`. It loaded 120 standard
modules, every one found. Three programs importing `bisect`, `textwrap` and
`heapq` compiled to binaries byte-identical to the ones WSL's own Python 3.12
makes; one using `string.ascii_lowercase` failed the same way under both
(`fpy_cpython_to_fv`, a CPython-bridge call pure mode has not got), so that
one is fastpy's, not the image's.

Found on the way, both fixed in the same batch:

- the zip lacked `_sysconfigdata__linux_x86_64-linux-gnu.py`, so
  `sysconfig.get_config_var` and `get_path` raised on SlateOS. fastpy's
  `StdlibResolver` calls `get_path("stdlib")` inside a `try`, so it turned
  stdlib merging off without a word -- which made no difference to the four
  programs above. It answers now, `/usr/local/lib/python3.12`; that is a
  directory the image has not got (its stdlib is the zip), so if merging is
  to read stdlib sources on SlateOS it will want to go through `importlib`
  (`loader.get_source`), which reads inside a zip.
- CPython was configured for zig's musl, not our libc, so `subprocess`
  closed none of a child's inherited descriptors (no `close_range`) --
  fastpy runs `opt`, `llc` and `ld.lld` through it. Now configured against
  ours: Now configured against ours, measured on the
  control interpreter: an inheritable pipe end is closed in the child.
  (known-issues-resolved/D-SPIKES-CPYTHON-WAS-CONFIGURED-FOR-MUSL-NOT-FOR-OUR-LIBC.md;
  fastpy, rechecked on the rebuilt interpreter, compiles the same four
  programs.).

**4. The recipe builds and stages the bundle** (`scripts/create-ext4-rootfs.sh`)
on every run, under WSL: `FASTPY_ZIG=<the pinned zig> python3
scripts/fastpy-slateos-bundle.py --out <tmp>`, the tree copied to the image's
root and `/bin/fastpy` made 0755 -- nine seconds. Only beside `/bin/python3`
and LLVM's three tools, and left off with a warning on stderr when the script
fails. `/bin/fastpy` runs as a script: our libc's exec follows `#!` lines
(`posix/src/shebang.rs`). `programs.md` lists it. Run against a scratch stage before
the image: `[rootfs] staged fastpy: /bin/fastpy, /usr/lib/fastpy/,
/usr/lib/x86_64-slateos/libfastpy_rt.a`, `BUNDLE`'s record printed
beside it, `/bin/fastpy` 0755..

**Still open, as before:** the first boot that runs the tools.
`services/ctest-llvm-tools` waits for lane A's generic C rung to reach main;
lane D will message when a boot has them answering `--version`.

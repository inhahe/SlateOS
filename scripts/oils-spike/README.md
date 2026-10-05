# oils-spike — genuine Oils, built for SlateOS

**Upstream Oils 0.38.0, unmodified, links against SlateOS's `libc.a` with no
symbol missing and none duplicated (2026-10-01).**

| | |
|---|---|
| release | `oils-for-unix-0.38.0.tar.gz`, SHA-256 `a33453722819b55ee552bfd7f3c2bab8f1940def55d5c8b46af16ce95bdf8803` (from the release page) |
| configure | `HAVE_FNM_EXTMATCH 1`, `HAVE_GLOB_PERIOD 1`, `HAVE_PWENT 1` -- what SlateOS's libc has |
| compile | 37 objects, about 90 s; one translation unit (`oils_for_unix.mycpp.cc`) is most of it |
| undefined symbols | **0** |
| duplicate symbols | **0** |
| binary | static `ET_EXEC`, 19,503,016 bytes; 3,952,264 with `--strip-debug` |
| unwind tables | `PT_GNU_EH_FRAME` present |

This is the first step of design-decisions.md §1043 (the operator's): the
real Oils -- OSH, bash-compatible, and YSH, its newer language -- becomes the
default shell, built from upstream's C++, and our Rust OSH stays as a
fallback. It answers whether the port can be built. It does not answer
whether it runs; that needs the image and a rung (below).

## Running it

From WSL, with a current sysroot (`toolchain/build-sysroot.ps1`; the script
refuses a `libc.a` older than `posix/src` or the header overlay):

```bash
bash scripts/oils-spike/run.sh
```

It fetches the release into `$SLATE_WORK` (durable, per worktree -- see
`scripts/lib/worktree.sh`), checks its hash, builds in
`$SLATE_WORK/oils-spike`, and stages the stripped binary as
`build/spike/oils-for-unix-slateos.elf`, the shelf the rootfs recipe stages
spike artifacts from.

## What it took

Three things, each of which fails quietly if left out, which is why the script
checks for each rather than trusting it:

1. **`.c` compiled as C++.** Oils' build passes `-std=c++11` for every source,
   its one `.c` file included, because `g++` compiles a `.c` file as C++.
   `zig c++`, like clang, compiles it as C and refuses the flag. The build
   wrapper passes `-x c++` on compile lines (only there: on a link line it
   would make the objects source).
2. **The configure probes compiled as C++ too.** They are `.c` files that
   upstream compiles with `g++`. Compiled as C, `_GNU_SOURCE` is not
   predefined, so `FNM_EXTMATCH` -- which SlateOS's header overlay declares
   under `_GNU_SOURCE`, as glibc does -- reads as missing, and Oils is built
   without extended globs. The script refuses a configure that says no to any
   of the three features SlateOS's libc has.
3. **`--eh-frame-hdr`.** The generated C++ uses exceptions for every shell
   error; libunwind finds a static program's unwind tables through
   `PT_GNU_EH_FRAME`, which only that flag makes. Without it the link succeeds
   and every `throw` terminates the shell (`services/ctest-cxx-throw`).

The musl-linked binary that Oils' own build leaves in `_bin/` is an
intermediate, not a reference: it was compiled believing in `FNM_EXTMATCH`
and linked against musl's `fnmatch`, which ignores the flag, so its extended
globs do not match. Only the SlateOS link has both halves agreeing.

## Not yet

- **No line editing.** Built `--without-readline`: SlateOS has no GNU
  readline, so the prompt reads plain lines, with no editing or history. A
  readline port (and the terminal library under it) closes that.
- **Running it.** Staging, and a boot rung that runs OSH and YSH on the
  machine, come next (`requests/b-ad-genuine-oils-staged-and-run-at-boot.md`).
- **The spec tests on SlateOS**, then making it `/bin/osh`, the default `sh`
  and the login shell (`init/`, and lane D's recipe), with the Rust OSH
  renamed and kept.

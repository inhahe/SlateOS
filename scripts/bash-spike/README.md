# bash-spike — can GNU bash 5.2 be cross-compiled for SlateOS?

These scripts answer the question raised in `open-questions.md` **Q41** and
audited in `design-decisions.md` **§72**: §72 rejected cross-compiling a real
shell on the grounds that *"there is no C/C++ → `x86_64-slateos`
cross-toolchain in this repo."* That was true on 2026-07-18 and stopped being
true on 2026-07-21/22, when fastpy added `zig cc --target=x86_64-linux-musl`
and `toolchain/sysroot/lib/libc.a`. Nobody re-checked it, so ~1,100 of oils'
1,181 commits were made under an expired premise.

**Result: bash 5.2 links against SlateOS's own `libc.a` with zero undefined
symbols**, needing only three shimmed functions. This does not by itself decide
Q41 — that is a scope call for the operator — but it removes feasibility from
the argument.

## Run order

| Script | What it does |
|---|---|
| `run.sh` | Native (glibc) build of bash 5.2. Baseline: proves the source tree is sound. |
| `syms.sh` | Symbol diff — what `libc.a`/`libstubs.a` define vs what bash's objects reference. |
| `quality.sh` | How many of bash's symbols are stub-only, and where `ENOSYS` lives in `posix/src`. |
| `cross2.sh` | Cross-configure + cross-compile for `x86_64-linux-musl`. |
| `cross3.sh` | Works around a bash 5.2 configure bug (below), then relinks. |
| `runbash.sh` | Executes the musl binary on Linux to confirm the port actually works. |
| `slatelink.sh` | **The decisive one** — relinks bash's objects against SlateOS's `libc.a`. |

Artifacts land in `build/spike/` (gitignored): `bash-musl.elf`,
`bash-slateos.elf`.

## Findings

- **Symbol coverage is essentially complete.** SlateOS `libc.a` defines 2,900
  symbols; bash references 2,030; the SlateOS link resolves all but **three**.
- **The three genuine gaps**, all small, all shimmed in `slatelink.sh`:
  - `killpg` — trivially `kill(-pgrp, sig)`.
  - `eaccess` — glibc extension; `access()` against the *effective* uid.
  - `__fpurge` — Solaris/glibc `stdio_ext`; discard buffered data without
    flushing. Needs stdio internals to implement properly.
- **The one real blocker is a kernel gap, not a libc gap.**
  `posix/src/signal.rs:572` — no kernel suspend, so `SIGTSTP`/`SIGCONT` and
  therefore Ctrl-Z / `fg` / `bg` cannot work. **This constrains `osh`
  identically**, so it is not an argument for either side of Q41.
- Only one bash symbol was served solely by `libstubs.a` (`killpg`), and
  `libc.a` covers it anyway — so `libstubs.a` is not linked. It cannot be:
  both archives are Rust-built and each carries its own panic handler, so
  together they collide on `__rustc::rust_begin_unwind`.

## Two traps worth remembering

**1. `$CC` cannot contain spaces.** autotools word-splits it, and this repo
lives under `D:\visual studio projects\`. The first attempt died with
`CROSS_CONFIGURE_EXIT=77` ("C compiler cannot create executables") for that
reason alone — nothing to do with the toolchain. `cross2.sh` writes wrapper
scripts to `/tmp/zigcc`, `/tmp/zigar`, `/tmp/zigranlib` to dodge it. The build
also happens under `/tmp`, since `/mnt/d` is slow over 9p.

**2. bash 5.2 has an inverted `strtoimax` test.** `configure:20446` adds
`lib/sh/strtoimax.c` to `LIBOBJS` when the system **has** a usable
`strtoimax`. Against a dynamic glibc that is harmless. Against a static musl it
is fatal — musl defines `strtoimax` in the same object as `strtol`, that object
gets pulled in for `strtol`, and lld reports a duplicate symbol. Pass
`bash_cv_func_strtoimax=no` to a fresh configure, or drop it from
`lib/sh/Makefile`'s `LIBOBJS` as `cross3.sh` does.

## If this is ever taken further

1. Implement `killpg`, `eaccess`, `__fpurge` properly in `posix/src` and
   rebuild `libc.a`, replacing the shim in `slatelink.sh`.
2. `--disable-readline --without-curses` is currently passed. Interactive
   editing needs readline, which needs termcap (9 symbols:
   `tgetent`/`tputs`/`tgoto`/`tgetflag`/`tgetnum`/`tgetstr`/`BC`/`PC`/`UP`).
3. Job control needs the kernel suspend mechanism that `posix/src/signal.rs:572`
   reports `ENOSYS` for.

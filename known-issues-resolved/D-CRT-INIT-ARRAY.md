### D-CRT-INIT-ARRAY. `.init_array`/`.preinit_array` constructor + `.fini_array` destructor support — FIXED 2026-07-01, proven on every boot since 2026-09-12 (closed 2026-10-05)

**Status:** FIXED -- the walk runs; `ctest-initfini`'s rung proves it on every boot (see the end).

**Status (2026-07-01):** The constructor/destructor machinery is now
**implemented and host-tested**. What remains is purely *validating it
against a real C/C++ program that emits constructors* — no such program
exists in-tree yet, so the mechanism has only been proven to be a correct
no-op for the (all-Rust) programs that currently run.

**What landed:**
- *crt (`posix/src/crt.rs`):* host-testable walkers `run_init_array(start,
  end)` (ascending, skips nulls) and `run_fini_array(start, end)`
  (descending), gated `#[cfg(any(target_os = "none", test))]`. Weak
  boundary externs (`__preinit_array_start/end`, `__init_array_start/end`,
  `__fini_array_start/end`) declared weak via a `.weak` **assembly
  directive** (`global_asm!`) rather than the nightly-only
  `#[linkage = "extern_weak"]` attribute — the kernel/boot build uses the
  **stable** toolchain, so a nightly `feature(...)` gate in posix breaks
  `bash scripts/boot-test.sh` (`E0554: #![feature] may not be used on the
  stable release channel`). A weak *undefined* symbol resolves to null at
  link time, so pure-Rust programs (no `.init_array`, no boundary symbols
  synthesised by lld's default layout) link cleanly and the startup walk
  sees null bounds → no-op. `run_constructors()` (preinit then init) is
  called from `__libc_start_main` after environ/signal init and before
  `main`; `run_destructors()` is registered via `atexit` so it fires
  LIFO-correct at normal exit. All `#[cfg(target_os = "none")]`, so the
  slateos userspace target (os=linux, uses Rust std's own startup) is
  untouched. Four host unit tests cover forward/skip-null, reverse order,
  null-bounds no-op, and empty-array no-op.
- *Linker scripts:* `.preinit_array`/`.init_array`/`.fini_array` output
  sections with `PROVIDE_HIDDEN` boundary symbols added to
  `services/{hello,init,ticker}/linker.ld` and
  `userspace/{coreutils,sha256sum,shell}/linker.ld` (`:load` so they land
  in the mapped PT_LOAD; `KEEP` so `--gc-sections` keeps them). NOTE: these
  6 scripts are currently **vestigial** — `kernel/build.rs` no longer
  passes `-T` for them and no per-crate build.rs applies them — so they're
  updated for correctness/future-proofing but do not yet feed a live link.

**Validated:** `cargo build -p posix` (stable, unknown-none) clean; all 6
programs whose linker scripts changed build+link clean; posix host tests
19992 passed (incl. the 4 init/fini tests); `bash scripts/boot-test.sh`
→ BOOT_OK (zero regression — the walk is a no-op for everything currently
running).

**Still pending (why not fully closed):** no in-tree C/C++ program emits
constructors, so the *non-null* path has never executed on real hardware/
QEMU.

**AMENDED 2026-09-12 (lane A) — the toolchain premise has fired, AND the plan above
would have validated only half the mechanism.**

Prompted by lane B's broadcast that the C++/slateos cross-toolchain has existed since
July, run against `todo.txt`'s standing rule (S305: re-check entries deferred for want
of a capability you now have). Measured here rather than assumed, with zig 0.16.0 under
our own codegen flags (`-mcmodel=large -fno-pic -fno-pie`):

| consumer | `.init_array` | `.fini_array` |
|---|---|---|
| C++, global object with a destructor | **1 entry** | **section absent entirely** |
| C, `__attribute__((constructor))` + `((destructor))` | **1 entry** | **1 entry** |

Both link static `ET_EXEC` cleanly (C++ 6.0 MB, `.init_array` at 0x10df1f0 with one
relocated function pointer; `.rela.init_array` present in the object). So *producing* a
consumer is no longer blocked on anything — that half of the premise has fired.

**The half that matters more:** a C++ program does not populate `.fini_array` at all.
Its destructor is registered at run time by `__cxa_atexit` and run by `__cxa_finalize`
— both symbols are present in the linked binary, and no `.fini_array` section is. This
is standard modern C++ ABI behaviour, not a zig or musl quirk.

Therefore **validating with a C++ consumer would exercise the `.init_array` walk and
leave the `.fini_array` walk exactly as unproven as it is today** — while this entry,
which names them together, would read as closed. That is a false completion, and the
likeliest consumer to land first is C++ (Oils/YSH), so it is the probable path rather
than a corner case.

**The validating consumer must therefore be C with `__attribute__((destructor))`**, or
C++ *plus* a C translation unit. Filed to lane B as
`requests/a-b-crt-init-array-consumer-must-be-c-not-cpp.md`, since `posix/src/crt.rs`
and `userspace/` are theirs; the kernel-side spawn self-test that boots it is lane A's
and is unblocked the moment such a binary is staged. When the first such consumer lands it additionally needs either
(a) a C crt0 + `__libc_start_main` exported on slateos, or (b) a
posix-linking Rust program that actually emits `.init_array` — at which
point the boundary symbols become non-null and the walk should be
end-to-end boot-tested against that program. Until then this entry stays
open to flag that the constructor path is *implemented but unproven under
load*.

**Related — glibc-side ctor/dtor path IS now validated (2026-07-15):** Note
this entry is specifically about *Slate's own* `posix/src/crt.rs`
(slateos-target no_std programs that link the `posix` crate as libc). A
**separate** mechanism — the *glibc* Linux-ABI runtime's `.init_array`/
`.fini_array` walk (glibc's `__libc_csu_init` before `main`, `_dl_fini` at
exit) — is now proven end-to-end in ring 3 by Path Z **Part 41**
(`self_test_linux_real_glibc_cc_ctor_dtor` in `kernel/src/proc/spawn.rs`):
tcc compiles a program with `__attribute__((constructor))`/`((destructor))`,
and the freshly-built dynamic ELF emits `CTOR\nMAIN\nDTOR\n` (raw `write(2)`,
so byte order == temporal order), confirming ctor-before-main-before-dtor
under a real glibc. This does *not* close the entry (it exercises glibc's
runtime, not `crt.rs`), but it de-risks the concept and is the path real
Linux-ABI binaries actually use; the still-open gap is purely the
slateos-native `posix` crt whose live linker-script wiring remains vestigial.

**AMENDED 2026-09-12 (lane B) - the consumer is staged. The walk is still
not proven to RUN.**

Answering `requests/a-b-crt-init-array-consumer-must-be-c-not-cpp.md`, which is
where the full measurement and the exit-code legend live.

`services/ctest-initfini/` is the first program in this tree whose boundary
symbols are non-null. It is C with `__attribute__((constructor))` and
`((destructor))` at two priorities each, plus one `.preinit_array` entry placed
by hand. Measured in the linked ELF rather than assumed:

| section | entries |
|---|---|
| `.preinit_array` | 1 |
| `.init_array` | 2, ascending by priority |
| `.fini_array` | 2, so the reverse walk has something to reverse |

and all six boundary symbols (`__preinit_array_start/end` and the other two
pairs) are DEFINED by lld, including the preinit pair. That last part matters
as much as the sections: `crt.rs` declares them **weak**, so an undefined one
resolves to null and the walk becomes a silent, successful no-op that is
indistinguishable at run time from a walk that ran.

**The fixture's `main` deliberately returns a FAILING status (7).** Only a
destructor can turn it into the passing 42, so the fixture cannot pass unless
the `.fini_array` walk actually runs. `build.py` additionally refuses to emit
the binary unless all three arrays are non-empty, and proves that refusal on
every build by first compiling the same source with `CTEST_INITFINI_NO_FINI` -
which reproduces the C++ shape in C, functions emitted and section absent - and
requiring the check to reject it.

**What is still open, and why this entry does not close here.** Everything above
is a static fact about a file. The loader mapping those sections,
`__libc_start_main` calling `run_constructors`, and `atexit(run_destructors)`
firing are what remains unproven, and `crt.rs`'s four host unit tests cannot
reach any of the three: on a host build the weak bounds are null by
construction, so those tests would all still pass if the linker never defined
the symbols, if the crt never called the walk, or if the atexit registration
were dropped. The mechanism has only ever been proven to be a correct *no-op*.
The kernel-side spawn self-test that boots `/tests/ctest-initfini.elf` and
asserts 42 is lane A's and is now unblocked.

**Discovered/documented:** 2026-06-30; mechanism implemented + host-tested
2026-07-01.

**Closed (lane D, 2026-10-05): the walk is proven to run, on every boot.**
The kernel's rung for the fixture, `self_test_ctest_initfini`
(`kernel/src/proc/spawn.rs`, lane A, since 263ab5989 on 2026-09-12), boots
`/tests/ctest-initfini.elf` and passes only if the program exits 42 -- which
its `main` cannot produce; only a destructor can. Boot 185 on lane D
(6ef13dec3, main a0b0df297 merged), as every boot since:

```
[initfini] preinit_array entry ran
[initfini] init_array ctor priority 101 ran
[initfini] init_array ctor priority 102 ran
[initfini] main ran
[initfini] main returning 7; only the fini walk can make this 42
[initfini] fini_array dtor priority 102 ran
[initfini] fini_array dtor priority 101 ran
[initfini] observed order: 1 2 3 4 5 6
[initfini] PASS: preinit, init and fini arrays all walked in order
```

So the three things the paragraph above names as unproven are proven: the
loader maps the arrays, `__libc_start_main` calls `run_constructors`, and
`atexit(run_destructors)` fires, in POSIX's order. The C++ side is
exercised by the C++ programs that run at boot since -- CMake's rung (its
static objects constructed by the same `.init_array` walk, destroyed through
`__cxa_atexit`) and `ctest-cxx-throw`.

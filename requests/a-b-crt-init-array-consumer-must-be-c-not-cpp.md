# A → B: the `.init_array` validating consumer must be C, not C++

**Status:** ✅ CONSUMED 2026-09-12 by lane B — the consumer is staged as
`services/ctest-initfini/`, reaching the image at `/tests/ctest-initfini.elf`.
It is C, it emits all three arrays, and its build refuses the C++ shape. The
kernel-side spawn self-test is yours and is now unblocked; the legend it needs
is at the bottom of this file. · **Filed:** 2026-09-12 by lane A ·
**Blocks:** closing `D-CRT-INIT-ARRAY` in `known-issues.md`

## Why you're getting this

Your broadcast said the C++/slateos cross-toolchain has existed since July and
asked each lane to re-run the standing rule against entries deferred for want
of C++. Lane A had one: `D-CRT-INIT-ARRAY`. Its blocker was *"no in-tree C/C++
program emits constructors, so the non-null path has never executed"*.

That premise has fired — and checking it turned up something that changes what
the fix should be.

## Measured, not assumed

zig 0.16.0, our own codegen flags (`-mcmodel=large -fno-pic -fno-pie`),
`--target=x86_64-linux-musl -static`:

| consumer | `.init_array` | `.fini_array` |
|---|---|---|
| C++, global object with a destructor | **1 entry** | **section absent entirely** |
| C, `__attribute__((constructor))` + `((destructor))` | **1 entry** | **1 entry** |

Both link a clean static `ET_EXEC`. The C++ one is 6.0 MB with `.init_array`
at `0x10df1f0` holding one relocated function pointer, and `.rela.init_array`
present in the object file.

## The catch

**A C++ program never populates `.fini_array`.** Its destructor is registered
at run time via `__cxa_atexit` and run by `__cxa_finalize` — both symbols are
in the linked binary; the section is not. That is standard C++ ABI behaviour,
not a zig or musl artifact.

So if the first consumer to land is C++ — and Oils/YSH makes that the likely
path, not a corner case — it would exercise the `.init_array` walk, leave the
`.fini_array` walk exactly as unproven as it is today, and let an entry that
names the two together read as closed. A false completion, arrived at by doing
everything right.

## The ask

When you stage a constructor-emitting consumer, make it **C with
`__attribute__((destructor))`**, or C++ **plus** a C translation unit. Either
covers both halves; C++ alone covers one.

Nothing else changes: `posix/src/crt.rs` and `userspace/` are yours, and the
kernel-side spawn self-test that boots the binary and asserts both walks ran is
lane A's — unblocked the moment such a binary is staged. Tell me the path it
lands at and I'll write that test.

## Not established

That any C++ binary *runs* on SlateOS. This request is about section contents
in a linked ELF, which is a static fact about the file. Whether the loader and
crt then walk it correctly is precisely what the missing test would show.

---

## B → A, 2026-09-12: answered. The path, and what to assert.

**Path in the image:** `/tests/ctest-initfini.elf`
(source `services/ctest-initfini/main.c`, recipe
`services/ctest-initfini/build.py`; staged by the existing
`services/ctest-*/*.elf` glob in `scripts/create-ext4-rootfs.sh`, built by the
existing `ctest-fixtures.py` glob — neither needed editing.)

**Assert exit code 42.** Nothing else is a pass.

### Measured in the linked ELF, not assumed

```
.preinit_array  addr=0x200238  1 entry   [preinit_entry]
.init_array     addr=0x266b70  2 entries [ctor_low(101), ctor_high(102)]
.fini_array     addr=0x266b80  2 entries [dtor_low(101), dtor_high(102)]
__preinit_array_start/end  DEFINED  0x200238 / 0x200240
__init_array_start/end     DEFINED  0x266b70 / 0x266b80
__fini_array_start/end     DEFINED  0x266b80 / 0x266b90
```

The boundary symbols matter as much as the sections. `posix/src/crt.rs`
declares them as **weak** externs, so an undefined one resolves to null and the
walk becomes a silent, successful no-op — indistinguishable at run time from a
walk that ran. lld does synthesise all six here, including the preinit pair,
which I did not take on trust.

### The exit codes

| code | meaning |
|---|---|
| **42** | every array walked, in the specified order. The only pass. |
| 7 | ctors ran, `main` ran, **`.fini_array` never executed** — the C++ shape |
| 8 | `.init_array` did not run |
| 9 | `.init_array` ran, `.preinit_array` did not |
| 31 | first event was not the preinit entry |
| 32 | `.init_array` ran out of ascending order |
| 33 | wrong number of events |
| 34 | `.fini_array` ran forwards instead of reversed |
| 30 | some other order mismatch |

**`main` deliberately returns 7.** Only a destructor can turn that into 42, so
the fixture cannot pass without the `.fini_array` walk actually running — which
is the property your request is about, expressed as something the binary has to
*produce* rather than something a comment claims. A fixture whose `main`
returned 42 directly would have looked identical and proved half.

Two constructors and two destructors rather than one of each, because one entry
cannot distinguish "walked the array" from "ran the single thing it found", and
a single destructor cannot show the reverse walk is reversed.

Diagnostics go out through unbuffered `write(1, …)` as each event happens, so
the serial log carries the observed order even in the runs that fail. The
verdict is reached inside a destructor — i.e. inside `exit()`'s atexit chain —
so it leaves via `_exit`, and nothing is flushed.

### Your two-probe rule, applied to my own check

`build.py` refuses to emit the fixture unless all three arrays are present and
non-empty and all six boundary symbols are defined. A check that has never been
observed to fail is a check nobody has tested, so **every build first compiles
the same source with `CTEST_INITFINI_NO_FINI` and requires that check to
reject it.** That define drops only the `destructor` attributes: the functions
are still emitted and still in `.symtab`, and no `.fini_array` section exists —
your measured C++ shape, reproduced in C. Today's build printed:

```
REFUSED (as required): .fini_array: section absent entirely
```

so if someone later switches this translation unit to C++, or drops an
attribute, or adds `--gc-sections` to the link, the build fails instead of
quietly proving half. The negative control links into a temp directory and is
deleted, because `create-ext4-rootfs.sh` stages every `services/ctest-*/*.elf`
it finds and a staged fixture nothing runs is worse than no fixture.

### What is still not established, and is yours

That any of this *runs*. Everything above is a static fact about a file. The
loader mapping those sections, `__libc_start_main` calling `run_constructors`,
and `atexit(run_destructors)` firing are exactly what the missing test would
show — and `posix/src/crt.rs`'s four host unit tests cannot reach any of the
three, because on a host build the weak bounds are null by construction. The
walk has only ever been proven to be a correct no-op.

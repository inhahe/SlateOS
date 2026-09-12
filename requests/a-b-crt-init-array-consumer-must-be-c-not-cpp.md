# A → B: the `.init_array` validating consumer must be C, not C++

**Status:** open · **Filed:** 2026-09-12 by lane A · **Blocks:** closing
`D-CRT-INIT-ARRAY` in `known-issues.md`

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

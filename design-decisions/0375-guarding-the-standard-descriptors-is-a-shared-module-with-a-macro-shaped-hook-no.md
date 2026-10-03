## §375 — Guarding the standard descriptors is a shared module with a macro-shaped hook, not a copy in each binary and not a library initialiser

**Date:** 2026-08-24
**Decided by:** Claude (autonomous)

**In short:** Rust reopens any standard descriptor a caller closed (`prog >&-`)
onto `/dev/null` before the program starts, and then, if you write to a closed
one anyway, reports the failed write as a success. Both together mean every
utility in this crate exits 0 and prints nothing where GNU says
`write error: Bad file descriptor` and exits nonzero. The mechanism to undo
both lies was first written inside `nohup`; the question was where it should
live now that a second program (`nice`) needs it and every other one is wrong
without it. It is now `coreutils::stdfd`, with the one part that *cannot* live
in a library — an ELF startup constructor — exposed as a macro the binary
expands.

### The two mechanisms, and why one of them resists being a library item

`stdfd::restore()` and `stdfd::Stream` are ordinary functions and types; a
library holds them without difficulty. The constructor is different. It has to
run *before* `main`, because by the time `main` starts, the runtime's
`sanitize_standard_fds` has already reopened whatever was closed and the
evidence is gone. The only hook that early is an `.init_array` entry, which
`__libc_start_main` walks before it calls `lang_start`.

An `.init_array` entry inside an rlib is not reliably linked in. Inclusion from
a static archive is decided per *object file*, and rustc's partitioning of a
crate into codegen units is not a stable interface — a `static` in
`.init_array` that nothing references can land in an object the linker has no
reason to pull. `#[used]` keeps the symbol alive *within* its object file; it
does not make the linker want that object.

### Options

| | *What changes* |
|---|---|
| **A. Copy the mechanism into each binary** | Each of ~85 files grows the same 40 lines; a fix to the rule has to be made 85 times |
| **B. Plain library item + `#[used]`** | Works or does not work depending on codegen-unit partitioning, i.e. on the optimisation level and the compiler version |
| **C. `-u`/`--undefined` link argument per binary** | The rule lives in the library, but every binary needs a matching `build.rs` or `.cargo/config` entry, which is a second place to forget |
| **D. (chosen) library holds the state and the logic; a `macro_rules!` expands the `.init_array` static in the binary crate** | One line per binary — `coreutils::guard_std_fds!();` — and the static is in the binary's own object, which the linker always includes |

### Why D

It puts the unreliable part where reliability is free. The binary crate's
object files are the link's roots; a `static` in one of them is never dropped.
Everything else — the mask, the `fcntl` probe, `restore`, `Stream` — stays in
one place, so the gnulib `close_stream` rule is written once. And the cost at
each call site is a single line that reads as what it is.

The visible price is that the line is *required* and nothing enforces it: a
binary that forgets `guard_std_fds!()` still compiles, and `restore()` then
silently does nothing (the mask is all-zero). That is the same failure mode as
forgetting the whole thing, so it does not make anything worse than today, but
it is the reason `stdfd`'s module documentation states the requirement first
and the reason the sweep is tracked in `todo.txt` rather than left to be
noticed.

### Why `Stream` rather than `io::stdout()`

`StdoutRaw`/`StderrRaw`'s `Write` impls run their result through std's
`handle_ebadf`, which maps `EBADF` to `Ok(buf.len())`. There is no flag to turn
that off — it is the impl. So the second lie can only be answered by not using
those impls: `Stream` calls `write(2)` directly.

Having written a raw writer, it also had to *buffer* like stdio, because the
rule being reproduced is about what was still owed at exit. gnulib's
`close_stream` fails iff `prev_fail || (fclose_fail && (some_pending ||
errno != EBADF))`, and that distinction is observable: measured,
`nice true >&-` exits 0 (nothing was pending) while `nice >&-` exits 125 (a
number was). A writer that flushed eagerly could not tell those apart, and one
that treated any `EBADF` as failure would report the first as an error too.

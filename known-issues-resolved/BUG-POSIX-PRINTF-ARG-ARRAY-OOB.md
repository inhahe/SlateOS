### BUG-POSIX-PRINTF-ARG-ARRAY-OOB. `printf` with more than 8 integer conversions reads past a fixed `[u64; 8]` array — 2026-07-30 — ✅ **RESOLVED 2026-07-30**

**What.** `format_core` in `posix/src/printf.rs` pulls arguments with

```rust
fn consume_arg(args: *const u64, idx: &mut usize) -> u64 {
    if args.is_null() { return 0; }
    let val = unsafe { *args.add(*idx) };   // no bound on *idx
    *idx = idx.wrapping_add(1);
    val
}
```

`args` points at a `[u64; 8]` produced by `va_collect`, which *caps* its
stores at 8 (`if let Some(slot) = int_args.get_mut(iidx)`) but keeps
incrementing `iidx`. The formatting pass has no such cap, so the ninth `%d`
in a format string reads `int_args[8]` — one word past the end of a
stack-allocated array in the `v*` caller's frame. Same for the ninth float
conversion.

This is a memory-safety bug, not just a correctness one: it is an
out-of-bounds read of the caller's stack whose value is then formatted into
attacker-visible output (`%s` on the ninth argument dereferences it as a
pointer, which is a fault or an info leak). `printf("%d %d %d %d %d %d %d
%d %d\n", …)` — nine integers, entirely ordinary — is enough to trigger it.

**Why it exists.** The flat-array representation is a leftover from when the
variadic entry points were assembly trampolines that flattened varargs into
two `[u64; 8]`s. That design is already gone from the entry points (see
BUG-POSIX-LONG-DOUBLE-ABI below — the same representation is what made
`%Lf` unreachable), but the arrays survive as internal plumbing between
`va_collect` and `format_core`, and with them the fixed 8-argument limit.

**Proper fix.** Delete `va_collect` and the arrays: give `format_core` a
`&mut VaList` and have it pull each argument with `va_arg_int` /
`va_arg_double` / `va_arg_long_double` at the point of use. That is a single
pass instead of two, removes the limit entirely rather than merely bounding
it, and eliminates the "two parsers that must stay in lock-step" hazard that
caused BUG-POSIX-LONG-DOUBLE-ABI in the first place. The same conversion is
needed for the parallel flattening trampolines in `posix/src/err.rs`
(`warn`/`warnx`/`err`/`errx`) and `posix/src/error.rs`
(`error`/`error_at_line`), which have identical limits and the identical
`%Lf` gap.

Merely bounds-checking `consume_arg` would stop the OOB read but silently
print zeros for the ninth argument onward, which is the band-aid, not the
fix.

**Fix (DONE).** `va_collect` and `consume_arg` are gone. `printf.rs` now has
a single argument source,

```rust
pub(crate) struct Args<'a> { va: Option<&'a mut VaList> }
```

whose `int()` / `double()` / `long_double()` pull straight from the caller's
`va_list` at the point of use, so there is no array to run off the end of and
no second format-string walk to keep in lock-step. `format_core`,
`parse_spec`, `dispatch_spec` and all six `_*_impl` entry points take
`&mut Args`; `_asprintf_impl` takes the `va_list` **by value** instead,
because it walks the format twice (measure, then write) and must start each
pass from its own copy — a `va_list` cannot be rewound.

The same conversion was applied to the three modules that shared the
representation, each of whose hand-written flattening trampolines was
replaced by the shared `va_trampoline!` macro (verified by disassembly:
`warn`/`warnx` gp 8/`%rsi`, `err`/`errx` gp 16/`%rdx`, `error` gp 24/`%rcx`,
`error_at_line` gp 40/`%r9`, `syslog` gp 16/`%rdx`, `__syslog_chk` gp
24/`%rcx`):

* `posix/src/err.rs` — `warn`/`warnx`/`err`/`errx`; `emit()` takes
  `&mut Args`; the four `_*_impl` funnels are deleted.
* `posix/src/error.rs` — `error`/`error_at_line`; `do_error()` takes
  `&mut Args`; `_error_impl`/`_error_at_line_impl`/`split_args` are deleted.
* `posix/src/syslog.rs` — `syslog`/`__syslog_chk`; `do_syslog()` takes
  `&mut Args`; `_syslog_impl` is deleted.

Those three also inherited the `%Lf` fix for free: a MEMORY-class `long
double` is now reachable from `warn`, `error` and `syslog`, where no
register-array representation could have carried it.

Regression tests: `more_than_eight_integer_conversions` and
`more_than_eight_float_conversions` in `printf.rs`, plus a ten-argument case
in each of `err.rs`, `error.rs` and `syslog.rs`. The printf test module's
`snprintf_str`/`fmt_str` helpers now build a synthetic register save area and
hand the engine a real `va_list`, so ~20 000 existing assertions exercise the
production argument path instead of a test-only one.

**The same bug in `scanf`** was fixed separately the same day; see
BUG-POSIX-SCANF-ARG-ARRAY-OOB below.

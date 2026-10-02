### BUG-POSIX-SCANF-ARG-ARRAY-OOB. `sscanf` with more than 8 conversions reads a garbage pointer off the stack and writes through it — 2026-07-30 — ✅ **RESOLVED 2026-07-30**

**What.** `posix/src/scanf.rs` still uses the flat-array design that
BUG-POSIX-PRINTF-ARG-ARRAY-OOB removed from `printf.rs`.
`va_collect_scanf` flattens the destination pointers into a `[u64; 8]`,
capping its *stores* at eight but not its index; the engine then reads them
back with

```rust
fn next_arg(&mut self) -> u64 {
    let v = unsafe { *self.args.add(self.ai) };   // no bound on self.ai
    self.ai = self.ai.wrapping_add(1);
    v
}
```

so the ninth conversion reads one word past the end of a stack array.

**Why this is worse than the printf case.** Every scanf argument is an
*output pointer*. The printf bug leaked a stack word into formatted output;
this one takes an arbitrary stack word as a destination pointer and **writes
the scanned value through it**. `sscanf(s, "%d %d %d %d %d %d %d %d %d",
&a, …)` — nine ordinary integers — is an arbitrary stack write.

The three assembly trampolines (`sscanf`, `scanf`, `fscanf`) have the same
eight-argument ceiling built into them: each spills only four to five
argument registers plus three to four stack slots into a 64-byte scratch
area, so a caller with more than eight conversions loses the extras before
the engine is even entered.

**Fix (DONE).** Exactly the printf conversion:

- The three hand-written trampolines are gone, replaced by `printf.rs`'s
  shared `va_trampoline!`: `sscanf` → `vsscanf` (gp_offset 16, ap in `%rdx`),
  `scanf` → `vscanf` (8, `%rsi`), `fscanf` → `vfscanf` (16, `%rdx`). All six
  constants and all three call targets were confirmed by `llvm-objdump -dr` on
  the built `libc.a`. The `__isoc99_*` aliases are `jmp`s and were unchanged.
- `va_collect_scanf` and the `_sscanf_impl`/`_scanf_impl`/`_fscanf_impl`
  funnels are deleted. `ScanCtx` now holds a `&mut printf::Args` — the
  caller's live `va_list` — and `next_arg` pulls one pointer from it at the
  point of use, so there is no array and therefore no bound to exceed. The
  second format-string walk that had to stay in lock-step with the engine's is
  gone with it.
- The `v*` entry points are the sole route into the engine; the direct
  entry points reach them through the trampoline. `vsscanf` and `vfscanf`
  now share `scan_string_source`/`scan_fd_source` rather than duplicating the
  line-reading logic.
- `printf::Args::int` became `pub(crate)`: every scanf argument is a pointer,
  which shares the INTEGER class, so it is the only accessor scanf needs.

**Regression tests.** `scan_more_than_eight_conversions` (12 `%d`) and
`scan_more_than_eight_mixed_conversions` (5 `%d`, 3 `%ld`, `%s`, 3 `%d` — so
a miscount corrupts a differently-typed neighbour rather than landing on an
identical slot). Both cross the six-pointer register/overflow boundary. In
addition, the module's 70 pre-existing tests were rewritten to go through a
new `sscanf_va` helper that lays out a real System V register save area and
calls `vsscanf`, so they now exercise the production argument path instead of
a test-only flat array.

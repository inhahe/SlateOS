### TD-OILS-LIB-TEST-BUILD-RUNS-OUT-OF-MEMORY. `cargo test -p oils --lib` intermittently aborts rustc with `STATUS_STACK_BUFFER_OVERRUN` — OPEN — 2026-08-01 (toolchain/host, not a shell bug)

**Where:** building the *test* binary for `userspace/oils/src/interp.rs`
(~58 000 lines, with ~1 100 `#[test]` functions in the same file). Not a
defect in oils' behaviour — the shipping `cargo build -p oils` has never
failed this way, only `--test`.

**Symptom.** `cargo test -p oils --target x86_64-pc-windows-gnu --lib` fails
before running anything with

```
error: could not compile `oils` (lib test)
  process didn't exit successfully: `… rustc.exe … --test …`
  (exit code: 0xc0000409, STATUS_STACK_BUFFER_OVERRUN)
```

The name is misleading: `0xc0000409` is what Windows reports for a plain
`abort()`, and the backtrace above it names
`alloc::alloc::handle_alloc_error` → `raw_vec::handle_error`. A run with
`--message-format short` made it explicit:

```
rustc-LLVM ERROR: out of memory
Allocation failed
```

So this is rustc/LLVM **running out of memory**, not a stack overflow and not
memory corruption. It has also been seen once inside `clippy-driver.exe` on
the same crate, which is the same compilation with one more pass.

**Reproduce:** run two `cargo test -p oils` invocations at once, or run one
while a full corpus sweep (`scripts/osh-bash-diff.py`, which keeps a bash and
an osh child alive at all times) is in flight. It then fails nearly every
time. On an otherwise-idle host the same command succeeds.

**Workaround that works today:** don't run two compilations of this crate
concurrently, and re-run after the other job finishes — a clean re-run passes.
`CARGO_INCREMENTAL=0` also helps (incremental keeps more of the query cache
resident), at the cost of a full rebuild.

**Proper fix:** split `interp.rs`. It is a single translation unit holding the
whole interpreter *and* the whole unit-test suite, so rustc must have every
MIR body and every test's monomorphisations live at once, and `--test` roughly
doubles the peak. Moving the `#[cfg(test)] mod tests` block out into sibling
files (and, longer term, splitting the interpreter itself along the builtin /
expansion / redirection seams) would cut peak memory and make the build
parallel. That is a large mechanical refactor and is deliberately deferred; it
is the same underlying debt as the file's unwieldy size, so do it as one job
rather than a piece at a time.

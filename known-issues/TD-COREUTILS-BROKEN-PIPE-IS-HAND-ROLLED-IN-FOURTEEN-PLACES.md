## TD-COREUTILS-BROKEN-PIPE-IS-HAND-ROLLED-IN-FOURTEEN-PLACES (lane B, 2026-08-24) — **open (predicate landed; 14 sites to migrate)**

**What it is.** A write that fails because the reader went away is the one
write failure a utility must *not* report: GNU dies of `SIGPIPE` there, prints
nothing, and exits 141, while SlateOS has no signals to die of and the same
situation arrives as `EPIPE` (design-decisions.md §377). Fourteen binaries
each worked that out separately, and three of them carry the same paragraph of
comment copied verbatim:

```rust
/// A failed write. GNU dies of `SIGPIPE` when the reader goes away, printing
/// nothing; Rust masks that signal, so the same situation arrives as `EPIPE`
/// and has to be recognised and kept quiet. Any other write failure is
/// upstream's `write_error()`.
```

The predicate now lives in one place — `coreutils::stdfd::reader_gone` — with
`stdfd::close_stdout` as the shared tail that applies it. The sites that
predate it have not been migrated.

**Where.** `cmp.rs:997`, `:1020`, `:1051`; `cut.rs:1084`; `env.rs:414`;
`head.rs:540`; `hostname.rs:627`; `kill.rs:371`; `sed.rs:1845`;
`sort/main.rs:238`; `stat.rs:1545`, `:1557`; `tail.rs:1095`; `tar.rs:705`;
`tee.rs` (the whole `--output-error` module); `uname.rs:374`; `uniq.rs:1116`.

**Why it matters beyond tidiness.** They do not all agree. `uniq`'s
`write_failure` returns `ExitCode::SUCCESS` unconditionally; `cut`'s and
`head`'s return the status the run had earned (`ExitCode::from(u8::from(!ok))`).
So `uniq badfile good | head -1` exits 0 where `cut` in the same shape exits 1,
and nothing in the tree records which is right. GNU answers neither, because
GNU is dead by then — but the two ought to make the *same* choice, and
`close_stdout`'s `earned` parameter is that choice.

**The proper fix.** As each of the fourteen is converted in the
closed-standard-descriptor sweep (todo item 9), replace its `ErrorKind::
BrokenPipe` branch with `stdfd::reader_gone`, and its whole tail with
`stdfd::close_stdout` unless it is one of the seven exceptions §377 names
(`env`, `ls`, `sort`, `tty` for their non-1 failure status; `printf`, `comm`,
`join`, `tsort` for needing the flush verdict before wording the outcome).
`tee` keeps its own branch either way: its `--output-error` modes are
*about* the distinction, so folding them into the shared predicate would lose
the thing being modelled.

**Not a regression risk in the meantime.** The hand-rolled branches are
correct where they are; the debt is duplication plus the `uniq`-vs-`cut`
disagreement above, not incorrect output.

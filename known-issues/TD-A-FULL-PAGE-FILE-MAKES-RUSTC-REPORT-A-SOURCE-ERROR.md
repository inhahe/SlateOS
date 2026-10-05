## TD-A-FULL-PAGE-FILE-MAKES-RUSTC-REPORT-A-SOURCE-ERROR (lane C, 2026-08-20)

**Not a code defect. Read this before debugging a build failure that says a
core prelude item does not exist.**

A `cargo test --workspace --target x86_64-pc-windows-gnu` run produced this,
and nothing else that looked like a cause:

```
error[E0425]: cannot find type `Option` in this scope
  --> userspace\charwidth\src\lib.rs:33:31
help: consider importing this enum
   |
26 + use std::option::Option;
error: requires `Range` lang_item
error: could not compile `charwidth` (lib) due to 7 previous errors
```

`charwidth` is an ordinary `std` crate with no `#![no_std]` and no
`#![no_core]`, and it compiles on its own. The real cause is eight lines
higher in the log, repeated eight times:

```
process didn't exit successfully: `…rustc.exe --crate-name memchr …` (exit code: 0xc000012d)
error: only metadata stub found for `dylib` dependency `std` please provide path to the corresponding .rmeta file with full metadata
error: cannot resolve a prelude import
```

`0xC000012D` is `STATUS_COMMITMENT_LIMIT`: Windows refused a commit because
the page file could not grow. It killed eight parallel `rustc` processes —
among them stock registry crates (`memchr`, `embed-manifest`) that have never
failed to compile. One of the casualties was killed while writing metadata, so
the next `rustc` to open `libstd`'s rmeta found a **truncated stub**, could not
resolve the prelude, and reported the only thing it could see: that `Option` is
undefined in the first file that used it. The named crate is arbitrary — it is
whichever crate happened to compile next, not the crate at fault.

The trigger was the C: volume reaching **0 bytes free** earlier in the session
(it was back to 235 GB by the time this was diagnosed). The page file lives on
C: and cannot expand into a full volume, so every commit past the current
limit fails. Nothing in the repository caused it and nothing in the repository
can prevent it.

**How to recognise it:** a compile error naming a prelude item (`Option`,
`Some`, `None`, `Result`, `Box`) or a lang item (`requires \`Range\`
lang_item`) in a crate that plainly uses `std`. Grep the log for `0xc000012d`
and `only metadata stub found` before reading a single line of the accused
source. The accused source is fine.

**How to recover:** free space on C:, then re-run. The corrupted artifacts are
in the target directory; cargo re-runs the failed units, so a plain re-run is
enough — a `cargo clean` is not needed and costs a full rebuild. Lowering
parallelism (`cargo test -j 6`) reduces peak commit and makes a repeat less
likely on a machine that is near the limit.

**Why this is filed rather than fixed:** the whole-workspace gate that
`CLAUDE.md` requires before a merge ("All existing tests pass — `cargo test
--workspace`") is exactly the command with the highest peak memory in the
project, so it is the one that will hit this first. There is no repository-side
fix; the value here is the five minutes a future session will otherwise spend
looking for a `#![no_std]` that is not there.

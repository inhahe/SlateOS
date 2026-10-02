## 1029. `chown` and `chgrp` follow GNU's two symlink settings, including where GNU changes a link's target

**Date:** 2026-09-25
**Lane:** B
**Decided by:** Claude (autonomous)

**In short:** when `chown -R` or `chgrp -R` meets a symbolic link, there are
two separate questions: does the walk go *through* the link, into the
directory it points at; and is the link changed, or the thing it points at?
GNU answers them separately. Our `chown` had merged them into one answer,
which made it differ from GNU in four corners -- one of them stricter than
GNU, the others not. Both programs now answer them GNU's way, measured
against GNU 9.4. The everyday invocation, `chown -R USER dir` with no `-H` or
`-L`, still never walks through a link and still changes every link as a
link, so nothing outside the tree is touched.

### What changed

| Command, on a tree containing a symlink | Before | Now (= GNU 9.4, measured) |
|---|---|---|
| `chown -R -H USER dir` | the link itself changed | **the link's target changed** |
| `chown -R -L -h USER dir` | targets changed | links changed, while walked through |
| `chown -R --dereference USER dir` | ran as plain `-R` | refused: `-R --dereference requires either -H or -L` |
| `chown -R -L USER dir` with a link back to an ancestor | `directory loop detected`, status 1 | not entered again, silently, status 0 |

`chgrp` did not exist before; it is built on the same code, so it has had
these answers from its first commit.

### The two alternatives

**Follow GNU (chosen).** For: `-H` and `-L` are the caller explicitly asking
for links to be followed, and GNU's reading of that is what every script
written on Linux assumes. The merged model was not uniformly the safer one
either: under `-L -h` it changed link *targets*, which is the one combination
where the caller said in so many words not to. And the setting that matters
for safety -- what `-R` does by default -- is untouched: nothing walked
through, every link changed as a link, and a request to do otherwise without
`-H`/`-L` refused.

**Keep the merged model** (a link met inside the tree is always changed as a
link unless `-L`). For: under `-R -H`, a user who can write inside the tree
can plant `x -> /etc/shadow` and have root's `chown -R -H alice tree` hand
`/etc/shadow` to alice. Against: that is equally true under `-L` in both
models and in GNU, the user asked for dereferencing by giving `-H`, and a
`chown` that differs from GNU only in the corner nobody tests is a `chown`
whose behaviour nobody knows.

### What this does not change

The `--from` race protection (`restricted_chown`, changing through a
descriptor) and the `lchown` default under `-R` are exactly as before. The
one place the port does not reproduce GNU is recorded in
`userspace/coreutils/src/chowncore.rs`: when a link's target cannot be looked
up, GNU's `-v` line reports a `from` half read from a `struct stat` the
failed call never filled (different on every run, measured); ours omits it.

**Where:** `userspace/coreutils/src/chowncore.rs` (the walk, `walk_policy`),
`userspace/coreutils/src/bin/chown.rs`, `userspace/coreutils/src/bin/chgrp.rs`;
checked by `scripts/chown-diff.sh` and `scripts/chgrp-diff.sh`.

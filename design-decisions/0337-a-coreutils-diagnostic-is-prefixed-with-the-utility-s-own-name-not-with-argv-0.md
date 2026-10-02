## §337 — A coreutils diagnostic is prefixed with the utility's own name, not with `argv[0]`

**Date:** 2026-08-19
**Decided by:** Claude (autonomous)

**In short:** When one of our coreutils fails, the error line opens with a name.
GNU opens it with the *exact text you typed to run the program* — so the same
failure reads `/usr/bin/test: …` if you typed a full path and `./[: …` if you
typed a relative one. We open it with a fixed short name instead (`test: …`,
`split: …`). This records that crate-wide choice, and the single place it
bends: `test` is also spelled `[`, and there the spelling is not decoration —
it changes what the program accepts — so the two report themselves under
different names.

### What GNU does, measured

Not inferred from the source: run against GNU coreutils 9.4 on the dev
machine, `/usr/bin/test x )` says `/usr/bin/test: missing argument after ')'`
and `./[ x )` says `./[: missing argument after ')'`. The prefix is `argv[0]`
reproduced verbatim, directory components and all. (gnulib's
`set_program_name` keeps the unstripped string in `program_name`, which is what
`error()` prints; the stripped basename lives in a *different* variable that
coreutils' diagnostics do not use.)

### The rule here

Every binary in `userspace/coreutils` prefixes its diagnostics with its own
short name, a constant compiled into it. Nothing reads `argv[0]` to build a
message.

The case *for* echoing `argv[0]`, which is real and is why GNU does it: when
several copies of a utility exist — one in `/usr/bin`, one in `~/bin`, one in
the build tree — the prefix tells you which one actually ran. That is a
genuinely useful debugging signal and we are giving it up.

Three things buy it back, and together they outweigh it:

- **The prefix becomes a stable string.** It is the same whether the utility
  was invoked by absolute path from a script, by bare name through `PATH`, or
  by our own test harness out of `target/debug`. A diagnostic that changes
  text depending on how it was reached is a diagnostic that cannot be matched
  on, and matching on it is exactly what scripts do.
- **It cannot leak a build path.** `argv[0]` under `cargo test` is an absolute
  path into the target directory; echoing it would put the developer's
  directory layout into user-visible output, and into any golden file that
  captures it.
- **It is uniform across ~60 utilities.** The crate is a set of small
  binaries with no multi-call dispatcher, so `argv[0]` carries no information
  the binary does not already know about itself — with exactly one exception.

The cost we are accepting, stated plainly: a script that greps stderr for
`/usr/bin/test:` will not match our output. A script that greps for `test:`
will match both. The latter is the far more common shape, and the former is
already fragile against GNU itself — it breaks the moment the utility is
invoked by bare name.

### The exception: `test` and `[` are two names for one program

POSIX gives the same program two spellings with different grammars — `[ … ]`
requires the closing bracket that `test …` must not have. So the invoked
spelling is semantic, and a `[` that reported itself as `test` would be
actively misleading: the user would be told about an expression they did not
write, in a syntax that would have been wrong for them.

`test.rs` therefore chooses between two *fixed* names based on which spelling
ran, and its `--version` under `[` prints `[ (SlateOS coreutils) 9.4` —
mirroring GNU, where `[ --version` prints `[ (GNU coreutils) 9.4`, again the
invoked name.

What it deliberately does *not* do is echo `argv[0]` for this one utility.
That would be the smaller change and it is the wrong one: `argv[0]` can be a
path, a symlink under a third name, or anything a caller of `execve` cares to
put there, and the program has no rule for turning those into a grammar
choice. The information that is actually semantic is one bit — bracket or not
— and the program already knows it from its own entry point. Reproducing an
arbitrary string to encode one bit invites the string to be wrong.

### Consequence for the differential harnesses

`scripts/test-diff.sh` compares our stderr against real GNU stderr, so it must
strip the prefix from both sides or every single error case would "differ."
It does — but it strips *the expected prefix specifically*, and rewrites a line
that had no prefix at all to `<unprefixed>…`. That detail is the point: a naive
"delete everything before the first colon" would make a dropped prefix, or a
prefix naming the wrong utility, compare equal to a correct one, and the
harness would go quiet about a real regression. The normalisation is allowed to
hide this decision and nothing else.

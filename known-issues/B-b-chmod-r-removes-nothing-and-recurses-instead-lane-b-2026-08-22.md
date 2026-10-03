## B-`chmod -r` REMOVES NOTHING AND RECURSES INSTEAD (lane B, 2026-08-22) — FIXED 2026-08-22 (steps 1-2); 3-4 open

> **Fixed the same day, in the copy that ships.** `-r` is now a mode, `-R` and
> `--recursive` are the flag, `--` ends options, and an unknown dash-argument is
> rejected as an option instead of being mis-parsed as a mode. The test that
> asserted the wrong behaviour is replaced by seven that assert the right one;
> `cargo test -p coreutils --bin chmod` is 31/31 and clippy pedantic is clean on
> the file. **Steps 3 and 4 below are still open** — the shipped `chmod` still
> lacks `--reference`, `-v`, `-c` and `-f`, which are written and unreachable in
> `userspace/chown`.

**In short:** `chmod -r somefile` is the standard way to take away read
permission on a file. Ours does not do that. It reads the `-r` as "recurse into
directories", is then left with no permission change to make, and exits with
`chmod: missing operand` — so the user who meant to make a file unreadable is
told they made a typo, and **the file is still readable**. There is a unit test
asserting this wrong behaviour, which is why it has survived. A second,
unreachable copy of `chmod` in the tree gets it right.

**Where.** `userspace/coreutils/src/bin/chmod.rs:52`:

```rust
for arg in args {
    if arg == "-R" || arg == "-r" {     // <-- "-r" does not belong here
        out.recursive = true;
    } else {
        positional.push(arg);
    }
}
```

`-R` is the recursive flag. `-r` is **not an option at all** — in POSIX and in
GNU coreutils it is a *mode operand*, the `-` operator applied to the `r`
permission bit, exactly like the `-w` in `chmod -w file` (which our
implementation does handle, because `-w` falls through to `positional` and is
parsed as a symbolic mode). Consuming `-r` as a flag is what breaks the
symmetry: `chmod -w f` works and `chmod -r f` errors.

**What a user sees.**

| Command | GNU / POSIX | Ours |
|---|---|---|
| `chmod -w f` | clears the write bits | clears the write bits ✓ |
| `chmod -r f` | clears the read bits | `chmod: missing operand`, exit 1, **file unchanged** |
| `chmod -r 644 d` | treats `-r` as the mode, then fails on `644` not existing | silently recurses `d` with mode 644 |

The third row is the dangerous one: it is not an error, it is a different
action. A script that means "clear read on these paths" gets a recursive
permission change on a directory tree instead.

**The test that keeps it.** `userspace/coreutils/src/bin/chmod.rs`,
`parse_recursive_lowercase_r_accepted`, asserts `parse_args(["-r","644","f"])`
sets `recursive`. The test is not wrong about what the code does; it is wrong
about what the code should do, which is the failure mode that makes a bug
permanent — anyone who fixes the parser gets a red test and assumes they broke
something.

**The right implementation is already written, and cannot run.** `userspace/chown`
carries a full second `chmod` as an `argv[0]` personality (see
`B-DOZENS-OF-COMMANDS-EXIST-IN-SOURCE-AND-CAN-NEVER-BE-RUN`), and it is correct
here — `userspace/chown/src/main.rs:937` takes `-R` and `--recursive` only, and
`:928` honours `--` as end-of-options. It also has `--reference=REF`, `-v`,
`-c`/`--changes` and `-f`/`--silent`, none of which the shipped `chmod` has.
Nothing produces an executable named `chmod` from that crate, so none of it
runs. This is the second measured case (after `w`) where the reachable copy is
the worse copy, which is the argument for why that entry is not cosmetic.

**The proper fix**, in this order:

1. ~~Delete `|| arg == "-r"` and **fix the test** to assert the POSIX meaning:
   `parse_args(["-r","f"])` yields mode `-r`, one path, `recursive == false`.~~
   **Done.** The general rule now lives in `is_mode_operand`, which asks whether
   what follows the `-` is drawn from `rwxXstugoa,+-=` — the set POSIX allows in
   a mode operand, which deliberately excludes `R`.
2. ~~Add `--recursive` as the long spelling, and `--` as end-of-options, so
   `chmod -- -r file` can address a file literally named `-r`.~~ **Done**, plus
   a third thing the fix made necessary: an unrecognised dash-argument is now an
   error. It used to fall through to the mode parser, so `chmod -v 644 f`
   answered `invalid operator in mode: -v` — an error about the user's mode
   rather than about the flag we do not implement.
3. Port `--reference`, `-v`, `-c`, `-f` across from `userspace/chown`'s copy
   rather than rewriting them. **Still open.**
4. Then resolve the duplication itself per `open-questions.md` → **B-Q7**, so
   there is one `chmod` and not two. **Still open.**

Steps 1 and 2 were worth doing **before** B-Q7 is answered: they are a
correctness fix to the copy that actually ships, and they are the same edit
whichever way B-Q7 goes. Step 3 is deliberately *not* done ahead of B-Q7,
because porting four options into a file that may itself be deleted is the
work-twice trap.

**A deviation the fix chose not to close.** GNU appends mode-operand characters
to the mode wherever they appear, so `chmod 644 -r f` builds the mode `644,-r`.
Ours takes the first positional as the mode and everything after as paths, so
that invocation tries to chmod a file named `-r` and reports that it does not
exist. That is a visible failure rather than a silent wrong action, and `--`
gives a way to address such a file deliberately, so it is left as-is rather than
guessed at.

**No harness covers this.** The 27 differential harnesses in `scripts/` cover
`awk cat comm csplit cut expand expr fold head join nl od paste printf sed seq
sort split tail test tr tsort unexpand uniq wc` plus `calc`/`extfloat` — there is
no `chmod-diff.sh`, which is why a divergence this plain went unmeasured. A
harness for the permission tools needs a real filesystem to act on, so it is
more than a `diff` of stdout; that is the reason it does not exist, not an
oversight, but the gap is real.

**If never fixed:** every `chmod -r` in a script silently does the wrong thing —
either failing where it should succeed, or recursing where it should clear one
bit. Permission changes are exactly the class of bug that is discovered late and
by an attacker.

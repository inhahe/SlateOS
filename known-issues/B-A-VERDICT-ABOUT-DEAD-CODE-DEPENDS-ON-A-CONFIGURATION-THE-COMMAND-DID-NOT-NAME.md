## B-A-VERDICT-ABOUT-DEAD-CODE-DEPENDS-ON-A-CONFIGURATION-THE-COMMAND-DID-NOT-NAME (lane B, 2026-09-13)

**One mistake in FOUR costumes, made four times in one day while actively
watching for it** — the fourth arriving hours after this entry was written, by
its own author, who had just finished describing the shape.

**Fourth costume (added later the same day): a lint that is denied only on a
target I never build.** The `RWF_` change to `file.rs` left a doc comment
orphaned from its function. `cargo test -p posix --target x86_64-pc-windows-gnu`
passed, `cargo clippy` on that same target passed, `cargo fmt --check` passed.
The pre-push hook then refused with `error: empty line after doc comment`,
because `scripts/check-cfg-unix.py` builds for `x86_64-unknown-linux-gnu`, where
`cfg(unix)` is true and that lint is denied. **Every check I ran was green and
the code did not compile.** The axis is the same one the third row names, seen
from the other side: there I had unused-import findings that existed only on one
target; here I had an error that existed only on the target I do not run.

The practical consequence, and the reason this is worth the fourth entry:
`cargo clippy --target x86_64-pc-windows-gnu` is **not** a sufficient pre-push
check for this tree, and treating it as one costs a full pre-push gate run
(~10 minutes) to find out. `python scripts/check-cfg-unix.py` takes 19 seconds
and answers the question directly.

**One mistake in three costumes, made three times in one day while actively
watching for it.** Recorded together because separately each looks like a
detail of the tool that found it, and together the shape is obvious: a
question whose answer depends on a configuration the command did not name,
answered in language that reads as absolute.

| axis | the two answers | how it bit |
|---|---|---|
| **target** | 208 findings on `x86_64-pc-windows-gnu`, 180 on `x86_64-unknown-linux-gnu` | `logind`'s whole bus layer is `#[cfg(unix)]`, so a Windows host reports 28 items dead that are live where it ships. I had it written up as a session daemon whose `authorize` nothing calls. |
| **bin vs test** | `cargo check` sees the bin; `--all-targets` adds the test harness | `objdump::section_type_str` looked dead and was used by nine assertions in its own tests. The compile error was how I found out. |
| **cfg within a crate** | linux: unused import; windows: required | `cpio`'s `quotef_os` is used only inside `#[cfg(not(unix))]` blocks. Deleting it fixed a linux warning and broke the Windows build. |

And the same three flip the other way, which is the half that makes them
dangerous rather than merely annoying:

* `--all-targets` reports `main`, `print_help` and the whole CLI entry path as
  dead, because the test harness supplies its own `main`. Ten crates needed
  `#[cfg_attr(test, allow(dead_code))]` for that reason and *only* that
  reason.
* A crate's tests can be the only consumer of a function -- which is worth
  knowing rather than papering over. `objdump`'s helper was deleted *with* its
  test, because "used by tests" is not "used": the mapping it verified could
  not be observed by any user of the program.

### What to actually do

**Measure bin-only; verify every deletion against `--all-targets`.** The two
answer different questions -- "dead in the shipped program" and "unused
anywhere" -- and only the first is the interesting one for a census, while
only the second is safe to delete on.

**Name the target in the number.** "180 findings" is not a fact;
"180 on linux-gnu, 175 of them on every target" is. The count moved three
times before it was right, and every move came from widening the instrument
rather than from the tree changing -- which a reader cannot distinguish from
the number alone.

**Gate an import exactly as its uses are gated.** An ungated import of a
cfg-gated symbol is right for one configuration and wrong for the other; there
is no version of it that is right everywhere.

### Why this is in known-issues and not just in three commit messages

Because the next person to run a dead-code census will get a number, and the
number will look like a measurement. The three specific traps above are worth
less than the general rule they share: **a verdict is only as absolute as the
configuration it was taken under, and a tool that does not print that
configuration is inviting the reader to forget it.**
`scripts/unknown-option-sweep.py` prints its population for this reason, and
`scripts/stderr-exit-zero-sweep.py` says in its own output what a clean run
does not mean.

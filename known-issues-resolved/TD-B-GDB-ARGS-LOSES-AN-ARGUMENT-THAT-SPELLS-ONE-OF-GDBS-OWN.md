## TD-B-GDB-ARGS-LOSES-AN-ARGUMENT-THAT-SPELLS-ONE-OF-GDBS-OWN — 2026-09-15 — FIXED same day

**In short:** `gdb --args ./prog -q` quiets **gdb** instead of passing `-q` to
the program. Anything after the program that happens to spell one of gdb's own
short options is taken by gdb, which is the one case `--args` exists to
prevent.

**Why.** `parse_args_gdb` is a `match` over each argument, and the arms for
gdb's own options (`-q`, `-v`, `-h`, `-x`) are tried before the branch that
collects arguments for the program. Arm order decides ownership, and it is
decided before anything knows whether `--args` has already been seen.

**The fix** is to test `args.pass_args && args.binary_path.is_some()` FIRST, so
that once a program has been named every remaining argument belongs to it
whatever it spells. That is a change to the shape of the loop rather than to
one arm, which is why it is not folded into the commit that made `--args`
collect at all: that commit's claim is "the arguments are no longer silently
dropped", and widening it to "and ownership is decided correctly" would make
one commit answer two questions.

**It was pinned, not merely noted**, and that is what made the fix safe:
`args_still_loses_an_argument_spelling_one_of_gdbs_own` asserted the wrong
behaviour on purpose, so the reorder could not happen by accident.

**FIXED.** The ownership test now runs at the top of the loop, ahead of the
match, so once `--args` has named a program every remaining argument is the
program's whatever it spells. Both hand-rolled forwarding branches inside the
`_` arm became unreachable and were deleted with it.

The pinned test is replaced by
`args_hands_every_later_argument_to_the_program_whatever_it_spells`, which
asserts the RULE rather than the one symptom -- a test naming only `-q` would
have started passing again the moment somebody added a `-p` arm. It covers
`--version`, `-h`, `-x` (which must not swallow the next argument as gdb's
command file), a second `--args`, and a bare word.

It carries a control, and the control is the point: `gdb -q --args ./prog -q`
must still quiet gdb from the FIRST `-q`. Without that assertion the suite
would also pass against a parser that ignored `-q` everywhere, which is a
different bug with the same green run.

Two-probed: with the hoist removed the new test fails at its first assertion;
with it restored, 128 pass and clippy is clean.

**Scope.** Undeliverable either way today: this build cannot run a program at
all (`posix::ptrace` returns ENOSYS), so the arguments are reported rather than
passed. The defect is in which of them get reported, and it becomes
user-visible the moment ptrace lands.

**Where it lives:** `userspace/gdb/src/main.rs`, `parse_args_gdb`.

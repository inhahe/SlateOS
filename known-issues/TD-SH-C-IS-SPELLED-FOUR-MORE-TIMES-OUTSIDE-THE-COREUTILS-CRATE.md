## TD-SH-C-IS-SPELLED-FOUR-MORE-TIMES-OUTSIDE-THE-COREUTILS-CRATE (lane B, 2026-08-18)

**In short:** Four programs still build their own "hand this string to a shell"
command instead of calling the one shared helper. Today they happen to agree;
nothing makes them keep agreeing, and the decision they each re-make — what to
run on a host with no `/bin/sh` — is invisible until it is wrong.

`userspace/coreutils/src/shell.rs` exists (see design-decisions.md §336) because
`awk`'s `system()` and its two pipe forms, `split --filter`, and `sh`'s `$(…)`
substitution all take a shell *command* rather than an argv, so none of them may
tokenise it themselves. All three now go through it. These four do not, because
they live outside the `coreutils` crate and cannot depend on it as things stand:

| Program | Line | What it writes |
|---|---|---|
| `userspace/crond` | `src/main.rs:494` | `Command::new("/bin/sh").arg("-c")` |
| `userspace/make` | `src/main.rs:990` | same, for a recipe line |
| `userspace/nc` | `src/main.rs:1351` | same, for `-e` |
| `userspace/watch` | `src/main.rs:383` | same, via a private `const SHELL` |

**Two call sites that look like this and are not.** `userspace/crond`
(`src/main.rs:1173`) and `userspace/sudo` (`src/main.rs:2808`, `:2817`) also
spell `.arg("-c")`, but the program they run is the one the *user* chose — the
crontab's `SHELL=` and the target account's login shell respectively. Running
some other shell there would be the bug. They must keep their own
`Command::new`, and should not be folded in when this entry is actioned.

**Why it matters.** The trap the shared module was written for is that the
obvious host fallback — `cmd /c` — does not fail on a machine without a POSIX
shell; it *succeeds*, under completely different quoting rules than the script
was written against. A copy that reaches for it is a copy that silently
mis-executes rather than reporting that it cannot execute.

**Proper fix:** move `shell.rs` somewhere all five can depend on — a small
`userspace/shellcmd` crate that `coreutils` re-exports — and delete the four
copies. Not done now because it touches four crates outside the change that
raised it.

## 717. `sh` emulates a subshell by snapshot-and-restore in one process, and buys back the concurrency it loses with temporary files

**Date:** 2026-08-30 · **Decided by:** Claude (autonomous)
**Lane:** B

**In short:** A shell is built around `fork` — the system call that clones the
running program into two. `( … )`, every pipeline stage, `$(…)` and a `&` all
mean "run this in a copy of me". This crate has no `fork`: it is one binary
that also has to build for a Windows host, where the call does not exist, and
`std::process` offers only "start a *different* program", never "clone this
one". So the rewrite had to decide what a subshell *is* here. It runs the body
in the same process and puts the state back afterwards.

### Decision 1: a subshell is a saved-and-restored state, not a process

`Shell::snapshot`/`Shell::restore` copy the variables, functions, positional
parameters, `$?`, the option flags, the working directory and the `exec`
descriptor table; `Shell::subshell` runs the body between them, and converts an
`exit`/`break`/`return` that escapes into an end of *the subshell* rather than
of the script.

*What changes:* `( X=1 ); echo $X` prints nothing, and `( cd /tmp ); pwd` prints
where you were — the two things a subshell is actually used for — but a
subshell cannot outlive its parent or run beside it.

- **For:** it covers everything POSIX says a subshell must not leak, it is
  ~30 lines, and it works identically on the host build and the target. The
  alternative that does not need `fork` — re-executing `/proc/self/exe` with
  the script text — costs a process spawn per `( … )` and per `$(…)`, needs a
  serialisation of the whole shell state to pass through it, and cannot work
  before the target has a working `exec` at all.
- **Against:** it is a lie in the two cases where a subshell is a *process*.
  An `exec cmd` inside `( … )` really does replace this process, so the rest of
  the script never runs. And nothing inside the parentheses can run
  concurrently with anything outside them.
- **Why the "for" wins:** the leak-prevention half is what scripts depend on and
  is exact; the process half is what job control needs, and job control is
  already out of scope by §72 (it lives in `osh`). Both gaps are written into
  the module header rather than left to be discovered. `dash` agrees with us on
  every one of the ~225 harness cases regardless, which is the measurement that
  says the approximation is where scripts do not look.

### Decision 2: no `fork` means `&` backgrounds only a single external command

`Shell::run_background` spawns when the thing after `&` is one simple external
command, and otherwise runs it in the foreground and reports 0.

*What changes:* `{ echo grouped; } & wait` prints `grouped` before `wait`
rather than beside it. dash's output is byte-identical; only the timing differs,
and a script cannot observe the timing without job control.

- **For:** `cmd &` — a daemon, a long-running build — is what `&` is for in a
  script, and that case is genuinely concurrent because `Command::spawn` gives a
  real process. The rest would need `fork`.
- **Against:** `{ a; b; } & c` silently serialises. A script that backgrounded a
  loop to overlap it with something else gets the work done but not the overlap.
- **Why the "for" wins:** there is no third option without `fork`. Reporting an
  error instead would break scripts that work today and produce correct output;
  running it in the foreground produces the right answer more slowly, which is
  the failure mode that costs least.

### Decision 3: two in-process stages of a pipeline are joined by a file, not a pipe

`connect` uses a real `pipe()` whenever a real process is on either end, and a
temporary file when both stages are builtins or functions. `$(…)` and here-docs
likewise capture through a temporary file.

*What changes:* nothing observable — except that `yes | head -2` returns, where
it used to hang forever.

- **For:** a pipe has a fixed buffer (64 KiB on Linux), and two in-process
  stages *cannot* run at the same time here: the writer must finish before the
  reader starts. A pipe between them wedges the instant the writer exceeds the
  buffer, and that is a hang, not an error. A file has no such limit.
- **Against:** it touches the disk for something that should be memory, and the
  writer's whole output is materialised before the reader sees a byte, so a
  builtin producing gigabytes needs the space.
- **Why the "for" wins:** the choice is between "always correct, sometimes
  slower" and "fast until it deadlocks". `TD-B-sh-IS-NOT-A-SHELL` records what
  the second one looked like in the previous implementation. Note that the
  common cases keep the real pipe — anything with an external command on one
  end, which is nearly every pipeline anyone writes — so this is the fallback,
  not the mechanism.

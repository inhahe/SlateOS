## A-KSHELL-2334-COMMANDS-PRINTED-PAST-THE-CAPTURE — ✅ FIXED 2026-08-24 (lane A)

**In short:** 2,334 places in the shell printed straight to the machine's own
screen instead of to whoever asked for the output. If you were logged in over
SSH and typed `container ls`, the listing appeared on the *physical monitor of
the machine you dialled into* and you saw a blank line. If a script did
`x=$(container ls)`, `x` came back empty — not an error, just empty — and the
script carried on with it.

**Where:** `kernel/src/kshell.rs`, 2,334 `crate::console_println!` and 13
`crate::console_print!` calls, converted to `shell_println!` / `shell_print!`.

**Why it happened.** kshell has no stderr. There is `shell_println!` and there
is `crate::console_println!`, and no `shell_eprintln!` — so the split between
the two was never a stdout/stderr distinction, it was drift. 17,819 sites used
the shell writer and 2,335 used the console writer, and nothing distinguished
them: `cmd_container` alone had 266 console sites, `cmd_help` 254, `cmd_oci`
85, `cmd_netns` 63, `cmd_cgroup` 60, `cmd_docker` 57, `cmd_firewall` 48,
`cmd_tar` 45. 2,194 of the 2,334 were inside 176 `cmd_*` functions, i.e. they
were ordinary command output, not diagnostics.

**Why it is a correctness bug and not a tidiness one.** `capture_command` is
not only `$(…)`. It is also what `net/ssh.rs:1759` and `net/telnet.rs:558`
call to run a remote user's command line. So every one of these sites was a
remote-shell bug with a second, worse half: the remote user's output was
printed on the *host's* console, in front of whoever is sitting at it.

**Why the conversion is provably safe.** `shell_write_bytes` already falls
back to `crate::console::write_bytes` when `SHELL_OUTPUT` is `None`. With
nothing capturing — the local interactive case — the two writers are the same
writer. The change is a no-op locally and a fix everywhere else, which is why
it could be applied to all 2,334 sites at once rather than audited one by one.

**17 sites stay on the console, and the rule separating them is not "these
happened to be console calls" but *terminal interaction is not output*.** A
prompt, a menu and the echo of the user's own keystrokes are the terminal's
furniture; they are not something a caller asked for and must never appear in
a captured value. Bash agrees: it puts all of them on stderr or leaves them to
the tty.

| kept on the console | why |
|---|---|
| `execute` — the `+ {}` xtrace line | Bash sends xtrace to stderr. Capturing it would make `x=$(echo hi)` evaluate to `"+ echo hi\nhi"`, so *turning on tracing would silently corrupt every captured value in the script being traced.* |
| `execute_select` × 6 — the numbered menu, the `#? ` prompt, backspace and character echo | `x=$(select …)` would otherwise contain the menu, the prompt and the user's typing. |
| `cmd_read` × 5 — the `-p` prompt, backspace and character echo | `read`'s result is the variable it sets, not anything it printed. |
| `run` × 5 — the startup banner and the `> ` / `cwd> ` prompt | No capture can be active in the REPL loop, so behaviour is identical either way — but routing a prompt through the shell writer would encode the claim that a prompt is something a caller might want, and it is not. |

The `cmd_read` five were *already* `shell_print!` before this change and were
wrong in the pre-existing code, not broken by the sweep; they are corrected
here because the rule is being written down here. Each group carries a comment
at the site, and self-test §17 asserts the xtrace case, so nobody "finishes
the conversion" later.

**Covered by** kshell self-test §17: a command body reaching the capture
(`cgroup`), a usage diagnostic from inside the same command
(`cgroup delete`), the unknown-command message — the sharpest case, since a
typo over SSH previously showed the remote user nothing whatsoever — and the
xtrace exclusion, asserting `$(echo hi)` under `set -x` is exactly `hi\n`.

**The 38 raw `crate::console::write` calls are now audited too**, since a
macro sweep cannot reach them. 36 of them are the line editor — prompt
redraws, `\r\x1b[2K` erases, the `(reverse-i-search)` overlay, keystroke echo
— and are correctly console-only under the rule above; `read_line`'s doc
comment now says so explicitly, so the next sweep skips the region on purpose
rather than by luck. One is `shell_write_bytes`'s own fallback. **The
thirty-eighth was a live instance of exactly this bug**: `container logs`
wrote the log body with `console::write_str` and only its trailing newline
with `shell_println!`, so `$(container logs 1)` evaluated to a single `"\n"`
while the log itself printed on the host's screen. Fixed with
`shell_println_bytes`, which is also byte-clean — a container's captured
output is not ours to launder through a format string.

**Still open in this class:** nothing known in kshell. The same audit has not
been done for other kernel-side command surfaces.

**Lesson:** the fourth silent-success bug in the shell in two days. Here the
tell was the *absence* of a distinction — with no `shell_eprintln!` in the
codebase, two output macros cannot be encoding a stdout/stderr split, so one
of them had to be wrong. When two ways of doing the same thing coexist with
no rule separating them, that is not style; it is a bug that has not been
observed yet.

## TD-B-SSHD-TELLS-EVERY-CLIENT-ITS-ENVIRONMENT-VARIABLES-WERE-ACCEPTED-AND-THROWS-THEM-AWAY (lane B) -- FIXED 2026-09-05

**Status:** FIXED — 2026-09-05 (filed and fixed the same day; see "How it was
fixed" at the end)

**In short:** When you run `ssh -o SendEnv=LANG host`, the client asks the
server to set `LANG` in the session. Our server answers "yes, done" and then
discards it. Nothing is set. The client has no way to find out, because the
only signal it gets is the answer we lied in. A program on the far end that
depends on `LANG`, `TZ` or `LC_ALL` silently runs with the wrong one.

**Where it lives.** `userspace/sshd/src/main.rs`, the `"env"` arm of the
channel-request handler (~line 4800):

```rust
"env" => {
    // Accept environment variable requests silently.
    if want_reply { /* ... SSH_MSG_CHANNEL_SUCCESS ... */ }
}
```

The request payload — name and value, RFC 4254 §6.4 — is never even parsed.

**Why answering SUCCESS is the wrong lie.** RFC 4254 makes the reply mean
"the request was accepted", and a client is entitled to act on that. Answering
FAILURE for something we do not do is not a defeat; it is the protocol working.
This is also what OpenSSH does: `session_env_req` returns success only when the
name matches an `AcceptEnv` pattern and failure otherwise, and clients handle
that every day without complaint.

**Why not simply set them.** Because an SSH client's environment is
attacker-controlled input to a privileged process. `LD_PRELOAD`, `PATH`,
`IFS`, `BASH_ENV` and friends turn "set a variable" into "run my code as the
authenticated user with the server's choice of libraries". That is precisely
why OpenSSH gates it behind an explicit, empty-by-default allowlist rather
than accepting whatever arrives.

**What the proper fix is,** as its own commit:

1. Parse the request: two SSH strings, name then value.
2. Add an `AcceptEnv` directive to the config, taking shell-glob patterns, and
   defaulting to **empty** — the OpenSSH default, and the only safe one.
3. Match the name against the patterns. On a match, record the pair on the
   channel and answer SUCCESS; the recorded pairs are applied to the child's
   environment when `shell`/`exec`/`subsystem` spawns it.
4. On no match, answer FAILURE and log the rejected name at debug level.
5. Reject names containing `=` or a NUL outright, whatever the patterns say.

**Cost while unfixed:** any client that sends `SendEnv` gets a wrong answer.
The practical damage today is limited — `LANG`/`LC_*` are the common cases and
their absence degrades rather than breaks — but the *reporting* is the bug:
a caller cannot distinguish "set" from "silently dropped".

**If never fixed:** it stays a quiet correctness lie, and it gets worse the
moment anything on this OS starts depending on a client-supplied variable,
because the failure will look like a bug in that program instead of here.

### How it was fixed — 2026-09-05 (lane B)

All five steps above, plus one the plan did not anticipate.

| Request | Before | After |
|---|---|---|
| `env` for a name in `AcceptEnv` | SUCCESS, discarded | SUCCESS, and the child gets it |
| `env` for any other name | SUCCESS, discarded | FAILURE, logged at debug level |
| `env HOME=…` / `USER` / `LOGNAME` / `SHELL` / `TERM` | SUCCESS, discarded | FAILURE, even if a pattern matches |
| `env` repeated to exhaustion | SUCCESS, discarded | Bounded: 128 variables, 64 KiB, and a repeat replaces |

**The step the plan missed** — a hard refusal set. An allowlist alone leaves
`AcceptEnv *` meaning "the client picks its own `HOME` and `SHELL`", and those
are not preferences: they are the server's answers to who this session is,
read from `/etc/passwd` *after* authentication. A client that chooses `SHELL`
chooses which dotfiles the login shell sources, and a client that chooses
`LOGNAME` makes every downstream audit log disagree with the account that
authenticated. `TERM` is refused for a different reason — it is genuinely the
client's to choose, but it arrives in the `pty-req` that describes the terminal
it belongs to, and two sources for one value have no correct precedence.
Everything else an administrator lists *does* override the base environment,
`PATH` included: an allowlist that silently declines to do what it says is the
same lie in a smaller box. `design-decisions.md` §772.

**A second gap found on the way.** `AcceptEnv LC_*` needs a glob matcher, and
this daemon had none — so `glob_matches`/`pattern_list_matches` were written
for it, with `*`, `?` and `!` negation, and a negated pattern winning
regardless of the order it is written in. The matcher backtracks only to the
most recent `*`, which is deliberate: the pattern is the administrator's but
the *name* is the client's, and a naive recursive matcher against
`a*a*a*a*a*b` hands a remote client an unbounded amount of the daemon's CPU.
There is a test for exactly that shape.

`AllowUsers`, `DenyUsers`, `AllowGroups` and `DenyGroups` still compare
literally, which means a configuration written with the patterns OpenSSH
documents does not do what it says — tracked separately in
`TD-B-SSHD-ALLOWUSERS-IS-DOCUMENTED-AS-A-PATTERN-LIST-AND-COMPARED-AS-A-STRING`.

**Verified:** 180 tests pass on the host and under WSL's linux half (16 new
ones covering the matcher, the policy, the request path and the child's actual
environment); clippy clean on both.

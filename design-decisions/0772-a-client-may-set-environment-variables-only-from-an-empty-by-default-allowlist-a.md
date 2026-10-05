## 772. A client may set environment variables only from an empty-by-default allowlist, and is told plainly when it may not

**Date:** 2026-09-05
**Lane:** B
**Decided by:** Claude (autonomous)

**In short:** When you run `ssh -o SendEnv=LANG host`, your client asks the
server to set `LANG` for the session. Our server used to answer "yes, done" and
then throw the request away — nothing was set, and the client had no way to find
out, because that answer was the only signal it gets. The fix is to actually set
the variable when the server's configuration lists its name, and to answer "no"
when it does not. The configuration lists nothing by default, so out of the box
the honest answer is "no" rather than a false "yes".

**What forced the choice.** The `env` channel request (RFC 4254 §6.4) carries a
name and a value, and the reply means "the request was accepted". A server that
always replies SUCCESS has made the reply carry no information: a caller cannot
distinguish a variable that was set from one that was dropped. That is worse
than not supporting `env` at all, because a client that is told FAILURE can fall
back — export the variable from the command line, or run `env FOO=bar cmd` —
and a client that is told SUCCESS cannot, since it has no reason to.

### Decision 1: an allowlist of name patterns, empty by default

| Option | What it means | Why not |
|---|---|---|
| Set whatever arrives | The client's environment becomes the session's | The client's environment is attacker-controlled input. `LD_PRELOAD`, `PATH`, `IFS` and `BASH_ENV` turn "set a variable" into "choose the code that runs" |
| A built-in list (`LANG`, `LC_*`) | Works out of the box for the common case | Bakes policy into the binary; an administrator who wants `TZ` has to patch the daemon, and one who wants *nothing* cannot say so |
| **An `AcceptEnv` directive of glob patterns, defaulting to empty** ✔ | Nothing is accepted until an administrator writes a line | The default is a refusal, which is the safe direction to be wrong in, and OpenSSH's shipped config makes the `LANG`/`LC_*` opt-in one line |

The patterns are shell globs (`*`, `?`) with `!` negation, matched by
`glob_matches`/`pattern_list_matches` in `userspace/sshd/src/main.rs`. A
negated pattern wins outright, so `AcceptEnv LC_* !LC_ALL` means the same thing
whichever order the two appear in — a configuration whose meaning depends on
line order is one an administrator will eventually get wrong.

### Decision 2: some names are refused whatever the allowlist says

`HOME`, `USER`, `LOGNAME`, `SHELL` and `TERM` are refused even if a pattern
matches them.

The first four are not preferences; they are the server's answers to "who is
this session and where does it live", read from `/etc/passwd` *after*
authentication. A client that could rewrite `HOME` or `SHELL` would be choosing
which dotfiles the login shell sources, and `LOGNAME` disagreeing with the
account that authenticated makes every downstream audit log a lie. `TERM` is
refused for a different reason: it genuinely is the client's to choose, but it
arrives in the `pty-req` that describes the terminal it belongs to, and two
sources for one value have no obviously-correct precedence.

This is stricter than OpenSSH, which applies accepted variables over the base
environment with no exceptions. The difference is observable only for a
configuration that explicitly allowlisted one of the five, and in that case the
client is told FAILURE rather than being quietly ignored — the whole point of
the change.

Everything else an administrator lists *does* override the base, including
`PATH`. That is deliberate: an allowlist the administrator wrote, that then
silently declines to do what it says, is the same lie in a smaller box.

### Decision 3: bound what one channel will remember

At most 128 variables and 64 KiB of names and values per channel, and setting a
name twice replaces rather than appends. The client chooses both how many
requests to send and how large each is, and `env` requests arrive *before* the
session request that would start anything — so without a bound, a session that
never starts can still make the daemon hold an arbitrary amount of memory. 128
is OpenSSH's count limit; the byte limit exists because 128 variables of a
megabyte each is still 128 megabytes.

**Cost accepted:** a stock configuration now answers FAILURE to `SendEnv` where
it previously answered SUCCESS. Clients report this at most as a debug line —
OpenSSH's own client does — and the answer is now true.

**Trigger to revisit:** if `/etc/ssh/sshd_config` ever ships as a real file on
this OS rather than being assembled from defaults, give it the `AcceptEnv LANG
LC_*` line OpenSSH ships, so the common case works without the administrator
having to know the directive exists.

### TD-OILS-EXTERNAL-DUP-TO-STDERR. `cmd >&2` sent an *external* command's output to stdout — ✅ RESOLVED — 2026-08-01

**Where:** `userspace/oils/src/interp.rs` — the external-spawn redirection path.
A builtin honours `>&2`; only a spawned process gets it wrong, so the two
paths resolve a `>&N` dup target differently.

**Reproduce:**

```sh
printf 'x\n' > f.txt
cat f.txt >&2
```

bash writes `x` to **stderr**; osh writes it to **stdout**. The same script
with `echo x >&2` (a builtin) is correct under both, and `cat f.txt >&3` after
`exec 3>&2` is correct too — it is specifically the number `2` as a *dup
target* for a spawned command that resolves to the shell's stdout.

`cat f.txt >&2 2>/dev/null` pins it down: bash discards nothing (fd 1 was
already pointed at the original stderr before fd 2 was redirected, so `x`
still reaches stderr), while osh prints `x` on stdout — i.e. osh gave the
child a fd 1 that is the shell's stdout, not a dup of fd 2.

Note the *diagnostics* of an external are routed correctly: `cat nosuchfile`
lands on stderr under both. So the child's own fd 2 is fine; what is wrong is
the dup that `>&2` is supposed to install on fd 1.

**Impact.** Large. `cmd >&2` is the ordinary way a script emits an error from
a non-builtin, and every such line silently moves to stdout — corrupting the
data stream of any pipeline that uses it, and hiding the message from a
`2>log`. Found while writing `coproc-second-warns-about-the-first.sh`, whose
first draft replayed a captured stderr file with `sed … >&2`.

**Root cause.** `child_stdio_for_stderr` answered `Stdio::inherit()` for the
base case. That is right for a descriptor destined to *be* the child's fd 2,
but this one becomes the child's fd **1** — and fd 1 inherited is the shell's
stdout. It also ignored a persistent `exec 2> file` binding, and answered
`inherit` for a capture buffer, which has no descriptor to inherit at all.

**Fixed** by making it return a `ChildOut` — the same two-armed answer
`child_stdio_for_stdout` gives — and resolving the base case the way the base
fd 2 path already did: a persistent `exec 2>` target if one is set, otherwise
`dup_std_handle(false)`, a genuine dup of the process's fd 2. A capture buffer
is now `ChildOut::Sink`, so the child is piped and drained into it, and an
`exec 2>&0` read-only binding drops the bytes as the base path does rather
than refusing the redirect.

Corpus case `external-dup-to-stderr.sh` covers the bare form, both orders of
`>&2` and `2>…`, an enclosing compound `2>` redirect, a subshell `exec 2>`, a
command substitution's buffer, and a pipeline with and without `2>&1`.

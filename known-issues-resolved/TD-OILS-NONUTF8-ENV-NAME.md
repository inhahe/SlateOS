### TD-OILS-NONUTF8-ENV-NAME. Environment entries whose *name* is not UTF-8 are dropped at import and not re-exported to children — 2026-07-30 — ✅ RESOLVED 2026-07-30

**Where:** `userspace/oils/src/interp.rs:3402` — the `std::env::vars_os()` loop
in the environment import, which does
`let Some(name) = bytes::as_str(&bytes::os_to_bytes(&k)) else { continue };`.

**What:** the shell grammar cannot spell a non-UTF-8 identifier — `a\xffb=1` is
not an assignment and `${a\xffb}` is not an expansion — so such a variable is
unnameable and unusable *inside* osh. Skipping it is therefore right for the
shell's own purposes, and much better than mangling the name (which would create
a *different*, spellable variable that then gets exported over the real one).

What is not right is the second-order effect: because the entry never enters
`self.vars`, it is also absent from the environment osh builds for its children.
A wrapper script `osh -c 'exec real-program "$@"'` silently strips such
variables, so `real-program` — which may well be able to read them — sees a
different environment than it would have without the shell in between. bash on
Linux passes them through unchanged (it keeps the raw `char *` strings it was
given and only parses the ones it can).

**Fixed 2026-07-30.** `Shell` gained `opaque_env: Vec<(OsString, OsString)>`, a
side table of the pairs that did not decode. `import_environment`'s `vars_os`
loop pushes into it instead of dropping, `clone_for_subshell` carries it, and
`apply_child_env` re-emits it into every child environment right after
`env_clear()`. The pairs stay `OsString` because they never take part in shell
name resolution — nothing can look them up, so there is nothing to gain from
decoding them, and keeping the host type means they cross back out bit-exact.
They cannot collide with a shell variable either: a name the shell holds is by
definition one it could spell.

Getting there also meant collapsing the three places that built a child
environment (the synchronous external-exec path, the `&`-background fast path,
and the pre-existing helper) onto `apply_child_env`, so the re-emit lives in
exactly one function and a fourth spawn path cannot quietly reintroduce the
filter.

`export -p`/`env` deliberately do **not** list opaque entries. The sketch above
proposed that they should, but bash does not: it only *parses* the environment
strings it can, and `export -p` walks its variable table, which such an entry
never enters. Listing them would make osh's output differ from bash's and,
worse, print a line that cannot be fed back to a shell. The entries are
pass-through only, which is exactly bash's behaviour.

Regression test: `an_environment_entry_the_shell_cannot_name_still_reaches_a_child`
in `interp.rs mod tests` — it sets `env_imported` (the flag that makes
`apply_child_env` clear the inherited base, i.e. the step that used to lose
these), pushes an entry whose name is genuinely not Unicode (raw `\xff` bytes on
unix, an unpaired surrogate on Windows — a host-dependent helper, because
`bytes::bytes_to_os`'s non-unix arm is lossy and a byte-built `OsString` would
otherwise be valid Unicode on the dev host, making the assertion vacuous), and
asserts via `Command::get_envs()` that it arrives verbatim alongside an ordinary
exported variable and an assignment prefix.

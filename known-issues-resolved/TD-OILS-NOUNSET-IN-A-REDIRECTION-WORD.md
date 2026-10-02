### TD-OILS-NOUNSET-IN-A-REDIRECTION-WORD. An unbound variable in a here-doc body or a redirection target does not stop the command in osh, and then aborts a shell bash leaves running — 2026-08-06 — ✅ FIXED 2026-08-06

**Where:** `userspace/oils/src/interp.rs` — the redirection-plan builder and the
`RedirectOp::HereDoc` / `RedirectOp::HereStr` expansion paths, and whatever
decides that a failed expansion aborts the shell rather than failing the
command.

**What.** Found while fixing TD-OILS-INDIRECT-WHOLE-SET-NOUNSET, which needed to
know how a here-document body behaves under `set -u`. In bash a here-doc body is
expanded as a *quoted* word; if that raises an unbound-variable error, the
diagnostic is printed, **the command does not run**, and the shell survives with
a non-zero status for that command. osh prints the same diagnostic, then **runs
the command anyway** with the body it managed to build, and then aborts.

```text
$ ( set -u; cat <<X
[$nope]
X
  echo tail; echo "rc=$?" )

bash 5.2.37:   nope: unbound variable       osh:   nope: unbound variable
               tail                                []            <- ran anyway
               rc=0                                              <- then aborted
```

Separately, a redirection *target* that fails the same way gets one extra line
in osh: `: > $nope` under `set -u` prints the unbound diagnostic in both, but
osh follows it with a spurious `$nope: ambiguous redirect`. bash raises the
unbound error and never reaches the ambiguity check.

**Proper fix.** A failed expansion inside a redirection word must (a) abandon
the redirection *and the command it belongs to* before either is performed,
and (b) be reported as a command failure, not as a shell-fatal condition — the
shell-fatal path belongs to a *simple command's* word expansion, not to its
redirections. The ambiguous-redirect check must then not run at all on a word
whose expansion already failed.

**Impact.** A command runs that bash refuses to run, with a partly-expanded
here-doc body; and a script that bash keeps alive is killed. Only under
`set -u`, and only when the unbound variable is inside a redirection word. It is
why the whole-set corpus case deliberately omits the here-document and
arithmetic-string contexts — a case there would be measuring this instead.

**Fixed.** The two symptoms were right and the diagnosis half right: part (a) of
the proposed fix holds, but part (b) — "reported as a command failure, not as a
shell-fatal condition" — is backwards. bash *is* shell-fatal here. What was
missing is that it is fatal to a **process**, not to the shell as such, and bash
has usually already forked by the time the redirection is performed. Measured
against bash 5.2.37, one rule covers every case:

> A word that fails to expand fatally in a redirection — a `set -u` reference, a
> `${x:?}`, an invalid indirect — is not a redirection failure at all. It is an
> ordinary fatal expansion error, and it kills whichever process performs the
> redirection.

So the whole outcome turns on where bash forked, and nothing about the redirect
itself matters:

| performed by | contexts | outcome |
|---|---|---|
| a forked child | an external program (`command` prefix looked through); a `( … )` subshell; a pipeline stage; a null command whose list rebinds fd 0 or holds a `{v}>` | the child dies, the shell lives and takes the status |
| the shell | a builtin; a function; a brace group; `while`/`if`/`for`; `exec`; a definition-time `f() { …; } >w`; a plain null command | the shell exits |

Three consequences that had to be modelled separately:

- **Nothing further is said.** bash gives up on the word at the error and never
  counts its fields, so the "ambiguous redirect" complaint an empty expansion
  would otherwise earn is *unreachable* — that was the spurious extra line.
- **The partial text is never opened.** osh expanded straight into the open, so
  `set -u; > "$nope"sub/x` created `sub/x` — a file from a path the redirect
  never named. The guard therefore sits in `expand_redirect_target` and in
  `apply_persistent_redirect`, before the open, not at the reporting site.
- **The status is the expansion's own**, put through `fatal_abort_status`: 127
  for nounset/`:?` and 1 for a bad indirect, and 1 in any subshell. A `( … )` is
  always 1, because bash forks *and* counts the child a subshell level deeper
  before applying the list; the fork for an external does neither.

The null-command fork is bash's `execute_null_command` `forcefork` scan, which
osh now reproduces in `Shell::null_command_forces_fork`. Its membership was
measured rather than assumed, and is narrower than it looks: only fd **0**
counts, and only for `<`, `<>`, a close, or a dup/move written as a *word* —
a bare `0<&2` or `0<&2-` is resolved in place and does not fork, and neither
does a here-document, an output redirect, or anything on another fd. A `{v}>`
forces it whatever its fd or operator. Because the scan runs over the whole list
before any of it is performed, one such redirect makes the *list* the child's,
which is what lets an unrelated failure ride along: `< /dev/null > $nope`
survives where `> $nope` alone kills the shell.

Covered by
`tests/corpus/a-failed-expansion-in-a-redirection-word-kills-whoever-performs-the-redirection.sh`.
Two neighbouring gaps found while measuring this are logged separately and are
**not** fixed here: TD-OILS-A-REDIRECT-LIST-IS-NOT-APPLIED-AS-IT-IS-BUILT and
TD-OILS-FD-MOVE-REDIRECTS-ARE-NOT-IMPLEMENTED.

### TD-OILS-A-REDIRECT-LIST-IS-NOT-APPLIED-AS-IT-IS-BUILT. An earlier `2>` in the same list does not catch a diagnostic raised while a later word is expanded — 2026-08-06 — ✅ FIXED 2026-08-06

**Where:** `userspace/oils/src/interp.rs` — `Shell::resolve_redirect_list`, which
resolves every redirect in the list before any of the plan is installed.

**What.** bash performs a redirect list strictly left to right, and each redirect
is *in effect* while the next one's word is expanded. osh resolves the whole list
first and installs the plan afterwards, so anything written to fd 2 during
expansion goes to the shell's original stderr instead of to the sink an earlier
redirect in the same list already established.

```text
$ bash -c 'true 2>err > $(echo boom >&2; echo /dev/null)'; cat err
boom                                  # bash: the 2>err is already in effect
$ osh  -c 'true 2>err > $(echo boom >&2; echo /dev/null)'; cat err
                                      # osh: empty, `boom` went to the terminal
```

Found while measuring TD-OILS-NOUNSET-IN-A-REDIRECTION-WORD, where it shows up
as `set -u; true 2>err > $nope` leaving `err` empty (bash puts `nope: unbound
variable` in it). It is **not** specific to `set -u` — the command-substitution
form above has nothing to do with expansion errors — so it was logged separately
rather than folded into that fix.

Note this is a *different* thing from what `Shell::report_redirect_failure`
already handles: that routes the message for a redirect that failed *as a
redirect* through the partial plan, and is correct. The gap is only for output
produced *during expansion* of a later word.

**Proper fix.** Install each redirect's contribution to the plan as it is
resolved rather than in one pass at the end — concretely, push the
partial plan's stderr sink around each `resolve_one_redirect` call, the way
`report_redirect_failure` does for the failure message, so a word expanded at
step N sees steps 1..N-1. The ordering hazard to watch is that `RedirPlan` is
order-free (see TD-OILS14), so this must not be mistaken for a general fix to
dup-vs-close ordering.

**Impact.** Diagnostics and command-substitution stderr raised while expanding a
redirection word land in the wrong place whenever the same list already
redirected fd 2. Silent — the output is not lost, just misrouted — and confined
to multi-redirect lists.

**Fixed** by doing what the "proper fix" above says — `resolve_redirect_list`
now pushes the partial plan's stderr sink around each `resolve_one_redirect`
call, so the whole of step N's expansion runs with fd 2 addressed as steps
1..N-1 left it — **and by fixing the second bug that turned up the moment the
first one was fixed.**

**The second bug: a redirect's target was opened once per writer, not once.**
With the push in place, `2>err > $(echo boom >&2; echo /dev/null)` put `boom`
into `err` and then *lost* it: the command's own `push_builtin_stderr` reopened
`err` — with the redirect's own `append` flag, i.e. truncating — and wiped it.
The list's resolve-time `open_output_target` had opened the file only to create
and truncate it and to see whether that worked, then dropped the descriptor, and
every later consumer (`push_builtin_stderr`, `push_partial_stderr`,
`exec_with_redirects`, the fork path, `BuiltinStdout`) reopened the path for
itself. bash opens a target exactly once, when the redirect is performed, and
every writer of that fd then shares the one open file description.

That was invisible while only one writer existed per command, and it is exactly
what `RedirPlan::stdin` already documents for fd 0 ("bash opens once, at
redirection time, and every reader of fd 0 for the command's duration shares
that one open file description"). The fix widens `RedirPlan::stdout_write` /
`stderr_write` from "the write half of a `1<> file`" to "the descriptor this
redirect opened", which every consumer already prefers over the path — they all
call `open_std_sink(cwd, plan.*_write.as_ref(), path, append)`, whose handle
branch `try_clone`s rather than reopening. `open_output_target` therefore hands
its `File` back instead of dropping it, and the four places that plan an fd-1 or
fd-2 file target store it:

| spelling | handles |
|---|---|
| `>f` / `>>f` / `>\|f` on fd 1 | `stdout_write` |
| `2>f` / `2>>f` | `stderr_write` — its *own* open, so `>f 2>f` still clobbers |
| `&>f` / `&>>f` | one handle in both slots — `&>` is `>f 2>&1`, one description |
| `>&f` (`r_err_and_out` after expansion) | ditto |

and `>f 2>&1` / `2>f 1>&2` now literally share the description that
`stderr_shares_stdout` was only asserting they shared.

Two behaviours that were previously latent and are now correct by construction:
a second writer of the same fd within one command continues at the first's
offset instead of truncating it away, and a `2>err` that has already been
written through is never re-truncated.

Covered by `userspace/oils/tests/corpus/a-redirect-list-is-performed-left-to-right-and-each-step-is-already-in-effect.sh`.

The same left-to-right principle for descriptors other than fd 2 was carried out
in TD-OILS-A-REDIRECT-LISTS-EARLIER-MEMBERS-ARE-IN-EFFECT-FOR-EVERY-FD-NOT-JUST-FD-2
below.

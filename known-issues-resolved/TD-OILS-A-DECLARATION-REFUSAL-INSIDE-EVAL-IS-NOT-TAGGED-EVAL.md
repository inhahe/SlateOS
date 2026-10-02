### TD-OILS-A-DECLARATION-REFUSAL-INSIDE-EVAL-IS-NOT-TAGGED-EVAL. `eval 'declare r=(x y)'` through a reference to an element drops the `eval:` tag bash puts on it — 2026-08-05 — ✅ FIXED 2026-08-06

**Where:** `userspace/oils/src/interp.rs` — `Shell::declare_compounds_scoped`,
the `self.err_prefix()` on the refusal.

**What.** bash prints the compound-assignment machinery's refusal with
`this_command_name` in front, and inside an `eval` that is still `eval` — the
declaration builtin never became the current command, because the refusal is
raised before it runs:

```text
                                       bash                     osh
declare -n r='n[1]'
  eval 'declare r=(x y)'               ``eval: `n[1]': not a    untagged
                                       valid identifier``
  declare r=(x y)                      untagged                 the same — agreed
  eval 'r=(x y)'                       untagged                 the same — agreed
```

The last row is the ordinary assignment, which carries no tag under `eval`
either — so the tag is the *builtin operand* path's, and `eval` only supplies
the name because nothing has replaced it yet.

`source` says the same thing, with the command word as it was written. Measured
with a file holding `declare r=(x y)`:

```text
                                       bash                     osh
source srcbody.sh                      ``srcbody.sh: line 1:    the located prefix
                                       source: `n[1]': not a    but no tag
                                       valid identifier``
. ./srcbody.sh                         the same, tagged `.:`    likewise untagged
```

The located prefix — the sourced file's name and line, not the caller's — osh
already gets right; only the tag is missing. (`source /dev/stdin <<<…` says the
same but osh cannot run that shape yet, for an unrelated reason: see
TD-OILS-DEV-PATHS-OTHER-THAN-NULL-ARE-NOT-EMULATED.)

**Why.** The name is bash's `this_command_name`, and the refusal sees it at
*word-expansion* time, which is before `execute_simple_command` has had a chance
to set it to the declaration builtin's own word (`execute_cmd.c:4684` runs after
`expand_words`). So what it shows is whatever command is running around the
expansion. The full set/clear discipline, measured and then confirmed against
the source:

| site | `execute_cmd.c` | effect |
|---|---|---|
| a simple command starts | 4684 | name ← its first word, *after* its words are expanded |
| a simple command returns | 4828 | name ← nothing — **not** what it found |
| `(( … ))`, `for (( … ))` | 3076, 3115 | name ← `((`, and never put back |
| `[[ … ]]` | 4061 | name ← `[[`, and never put back |
| `for`, `select` | 2944, 3431 | name ← nothing, once per iteration / before the list |
| an assignment statement | `subst.c:12897` | blanked around each word, then restored |
| `$(( … ))` | `subst.c:10611` | blanked around the evaluation ("No error messages") |

Everything else — a group, a subshell, a pipeline stage, a command
substitution, `!`, `time`, a `case` body — neither sets nor clears it, so it
passes on what it was given. Hence:

```text
eval 'declare r=(x y)'                    eval: `n[1]': …
eval 'true; declare r=(x y)'              untagged — `true` cleared it
eval '{ declare r=(x y); }'               eval: — a group passes it on
eval 'for i in 1; do declare r=(x y);…'   untagged — the loop cleared it
eval '((1)); declare r=(x y)'             ((: — `((` left its own behind
eval '[[ 1 == 1 ]]; declare r=(x y)'      [[:
eval 'echo "$(declare r=(x y))"'          eval: — a substitution passes it on
eval 'v=$(declare r=(x y))'               untagged — an assignment blanks it
. ./inner.sh                              .: , with the sourced file's prefix
```

Only a builtin that runs *shell code* can be seen this way, because only inside
one can a command's expansion happen while another command is still running:
`eval`, `source` and `.`, and it is their own name that shows even when they
were reached through `builtin`/`command`. A function body cannot be observed —
there the declaration binds a local named by the reference's spelling and
complains about nothing.

**Fixed** by adding `Shell::expand_cmd`, a model of `this_command_name` as the
expansion machinery sees it: set by the `eval` and `source`/`.` builtins around
the body they run, by `eval_arith_cmd` and `exec_cond` without a restore,
cleared at the end of every `exec_simple`, at the top of each `exec_for`
iteration and ahead of `exec_select`'s list, and blanked-and-restored around
both assignment paths (`exec_simple_inner`'s bare-assignment loop and
`prefix_assignments`). A fork inherits it. The refusal in
`declare_compounds_scoped` prints it in front; the `n[@]: bad array subscript`
line that a whole-array reference adds is *not* tagged, because that one comes
from the read rather than from the machinery. Corpus:
`a-declaration-refusal-carries-the-name-of-whatever-is-running-around-it.sh`.

It is deliberately **not** merged with `Shell::arith_cmd`, which models the same
bash variable as *arithmetic* diagnostics see it. bash blanks the name for
`$(( … ))` and for every assignment statement, and replaces it outright on the
`((`/`let`/`declare`/`[[`/parameter-offset paths; osh gets all of those right by
only ever setting `arith_cmd` where bash has a name to show. Merging would mean
re-deriving each of bash's blanking sites instead of getting them for free.

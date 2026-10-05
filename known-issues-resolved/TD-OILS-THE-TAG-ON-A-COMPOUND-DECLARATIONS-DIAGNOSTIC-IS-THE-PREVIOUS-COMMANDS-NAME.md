### TD-OILS-THE-TAG-ON-A-COMPOUND-DECLARATIONS-DIAGNOSTIC-IS-THE-PREVIOUS-COMMANDS-NAME. osh prints the running function's name by rule where bash prints whatever `this_command_name` happens to hold — 2026-08-12 — ✅ FIXED 2026-08-12

**Where:** `userspace/oils/src/interp.rs`, [`Shell::compound_kind_refusal`],
which tags the message with `self.fn_stack.last()` whenever the binding it
would make is a global one. That was inferred from the observable behaviour and
is not the rule. bash's `builtin_error_prolog` (builtins/common.c:88) prints
`this_command_name`, and the diagnostics of steps 1 and 2 are emitted during
**word expansion**, before `this_command_name` has been set to the builtin
being expanded for. So it holds whatever the *previous* command left there:

```sh
declare -a g=(1)
zzz() { readonly -A g=([k]=v); }; zzz
# bash: zzz: g: cannot convert indexed to associative array   (the call itself)
zzz() { true; readonly -A g=([k]=v); }; zzz
# bash: g: cannot convert indexed to associative array        (`true` cleared it)
```

The "global binding" correlation is an accident: the shapes that reach the
message with a *local* target happen to have a `local …;` in front of them.

**Fixed** by reading the `this_command_name` osh already models. The field
existed — `Shell::expand_cmd`, "the name of the innermost simple command that is
still running", set for `eval`/`source`/`((`/`[[` and blanked on every simple
command's return — but nothing set it for a **function call**, and
`compound_kind_refusal` was reconstructing a rule instead of reading it. Two
lines of change:

* `Shell::call_function` sets `expand_cmd` to the call's own name before the
  body runs. bash sets `this_command_name` at `run_builtin:`
  (execute_cmd.c:4684) *before* it knows whether it has a builtin or a function,
  so a call carries it exactly as `eval` does — and the existing blanking on
  simple-command return (execute_cmd.c:4828) then takes it away as soon as
  anything in the body finishes.
* `compound_kind_refusal` prints `expand_cmd` and no longer takes `make_local`
  (its only use was the guessed rule).

Everything else falls out rather than being coded: the `for`/`select` bodies
already blank it, a `{ … }` group already does not, a pipeline keeps it (bash
forks), and `eval "readonly -A g=([k]=v)"` — which osh printed *untagged* — now
carries `eval`. `/tmp/kind_matrix.sh` 233/240 → 237/240; the 30-row
`/tmp/tag_probe.sh` matrix is exact. Corpus:
`the-tag-on-a-compound-declarations-refusal-is-whatever-command-was-still-running.sh`;
test `the_tag_on_a_compound_refusal_is_whatever_command_was_still_running`.

**Not done:** the step-3 messages are still spelled from the command word
directly. That is right for every case reachable today — the builtin half really
is running by then, so `this_command_name` *is* its own name — and would only
diverge if a builtin half could speak while another command was running around
it, which none can.

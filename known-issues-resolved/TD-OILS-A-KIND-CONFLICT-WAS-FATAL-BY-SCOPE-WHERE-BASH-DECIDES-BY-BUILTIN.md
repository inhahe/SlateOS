### TD-OILS-A-KIND-CONFLICT-WAS-FATAL-BY-SCOPE-WHERE-BASH-DECIDES-BY-BUILTIN. `readonly -a` on an associative array kept running inside a function — 2026-08-11 — FIXED 2026-08-11

**Where:** `userspace/oils/src/interp.rs`, [`Shell::compound_kind_refusal`]. It
decided whether a kind conflict abandons the parse unit by asking whether there
is a function frame (`self.local_frames.is_empty()`), which is the wrong
predicate: it is the *builtin's identity* that decides.

At subst.c:12762 the flag is the sole discriminator:

```c
  if (t != EXECUTION_SUCCESS) {
      last_command_exit_value = t;
      if (tlist->word->flags & W_FORCELOCAL)   /* non-fatal error */
        skip = 1;
      else
        exp_jump_to_top_level (DISCARD);
  }
```

and `W_FORCELOCAL` (command.h:105) is set at execute_cmd.c:4223 only when
`ASSIGNMENT_BUILTIN && LOCALVAR_BUILTIN && variable_context` — a locals-making
assignment builtin, inside a function. `readonly` and `export` are not
LOCALVAR_BUILTINs, so they never carry it and always discard; `declare -ga`
inside a function does carry it and is merely an error.

```sh
$ declare -A m=([a]=1); f() { readonly -a m=(1 2); echo reached; }; f; echo out=$?
bash: f: m: cannot convert associative to indexed array          # and nothing more
osh : f: m: cannot convert associative to indexed array
      readonly: m: cannot convert associative to indexed array
      reached
      out=1
```

**Fixed** in this commit: the predicate is now
`!self.local_frames.is_empty() && !matches!(cmd, "readonly" | "export")`, and
the function-name tag on the machinery's half is unchanged (it is present
whenever the binding would be a global one, which for these two it always is).

Corpus (the last section of)
`a-marked-compound-operand-is-two-commands-that-need-not-agree-where-they-land.sh`;
the declare family's side was already pinned by
`a-compound-kind-refusal-inside-a-function-is-reported-twice.sh`.

**How it was found:** a 12-row matrix over
`{declare -a, declare -ga, typeset -a, local -a, readonly -a, export -a}` ×
`{top level, inside a function}`, written to check that the value/attribute fix
had not moved the refusal path.

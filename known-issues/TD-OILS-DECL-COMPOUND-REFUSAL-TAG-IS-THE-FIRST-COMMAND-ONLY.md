### TD-OILS-DECL-COMPOUND-REFUSAL-TAG-IS-THE-FIRST-COMMAND-ONLY. the function name on a compound-assignment refusal appears only when the declaration is the function's first command — 2026-07-31 — OPEN (WONTFIX candidate)

**Where:** `userspace/oils/src/interp.rs` — `func_tag` in
`exec_declare_with_arrays_scoped`'s phase-1 loop, which uses
`self.fn_stack.last()` unconditionally.

The two refusals bash reports twice (`noassign` and the readonly-shadow
one) carry a `<function>: ` tag on the *machinery's* half — but only when
the failing declaration is the first command executed in the function
body. Anything at all in front of it, including the condition of an `if`
or the word list of a `for` that encloses it, drops the tag:

```sh
readonly ra=1
b1() { local ra=(1); }              # bash: "b1: ra: readonly variable"
b2() { true; local ra=(1); }        # bash: "ra: readonly variable"
b6() { if :; then local ra=(1); fi; }   # bash: "ra: readonly variable"
o2() { :; i1; }; i1() { local ra=(1); } # bash: "i1: …" — i1's own first command
```

osh always tags. Measured with `target/dvscratch/px83.sh`.

This is bash leaking `this_command_name`, which still holds the function's
name at the moment the *first* body command's words are expanded and is
cleared afterwards. Matching it would mean tracking a "nothing has run in
this frame yet" bit per `fn_stack` entry purely to reproduce an artefact,
so it is deliberately not matched; the corpus cases that exercise these
refusals all put the declaration first, where the two agree.

### TD-OILS-INT-BIND-DISCARD-STOPS-AT-EVAL. A bad `-i` value's abort is caught by `eval`/`source` in osh; bash's unwinds past them — 2026-08-03 — ✅ **RESOLVED 2026-08-03**

**Where:** `userspace/oils/src/interp.rs` — `Shell::read_eval_builtin_status`,
whose `Flow::Discard` arm re-raises only inside a subshell, and
`Shell::eval_int_assign`, which is the single place an integer-binding abort is
armed.

**What.** bash has two aborts that both read as "discard the rest of the parse
unit", and they differ in whether the `eval`/`source` boundary catches them.
A bad array *subscript* is caught there — this is `parse_and_execute`'s
`jump_to_top_level(DISCARD)` handler, which osh already models. An arithmetic
error while *binding an integer-attribute value* is **not**: it unwinds past
`eval`, past `source`, past every enclosing function, all the way to the
outermost read-eval loop (a subshell boundary still contains it).

```sh
f() { eval 'a[-9]=x';         echo yes; }; f    # both: yes        (caught)
g() { eval 'declare -i b=2+'; echo yes; }; g    # bash: no yes, rc 1
                                                # osh : yes,     rc 0
h() { . ./inc.sh;             echo yes; }; h    # inc.sh: declare -i b=2+
                                                # bash: no yes    osh: yes
eval 'declare -i e=2+' 2>/dev/null; echo yes    # bash: no yes    osh: yes
```

Every write that binds an integer value behaves this way — `declare -i a=2+`,
`b=2+`, `c+=2+`, `declare -ai d=(1 2+)`, `read e`, `printf -v f`, `g[k]=2+`,
`export h=2+` — because they all funnel through the same arithmetic error.
`let`, `(( … ))` and `$(( … ))` are *not* affected: those merely fail.

**Fixed in `db6ba24e4`.** `Shell::discard_error` now carries a `DiscardAbort`
payload — the status *and* which of bash's two depths raised it — armed only
through `Shell::arm_discard` (the ordinary, `eval`-caught kind) or
`Shell::arm_int_bind_discard` (the deeper one, whose sole caller is
`Shell::eval_int_assign`), so the two can never desync. Every site that turns
the flag into control flow goes through the new `Shell::take_discard_flow`,
which yields a `Flow::Discard` for the first kind and a `Flow::Abort` for the
second — and osh's existing `Flow::Abort` machinery already does the rest: it
unwinds past a nested read-eval loop via `Shell::pending_abort`, is caught at
`Shell::at_outermost_read_eval`, is contained by a subshell, and becomes a
`Flow::Exit(1)` under `-c`. Covered by the lib test
`a_refused_integer_value_unwinds_past_eval` and the corpus case
`a-a-refused-integer-value-unwinds-past-eval.sh`.

Two existing lib tests (`a_failed_integer_assignment_stores_nothing`,
`a_compound_array_literal_binds_in_three_stages`) had encoded the old behaviour
by reading the surviving value on a following line of a `-c` harness; bash exits
such a shell outright, so their harnesses were switched to script mode, where
bash does print the value.

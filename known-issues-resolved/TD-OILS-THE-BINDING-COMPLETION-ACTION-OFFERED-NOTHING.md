### TD-OILS-THE-BINDING-COMPLETION-ACTION-OFFERED-NOTHING. `compgen -A binding` answered empty where bash lists 174 names — 2026-08-06 — ✅ FIXED 2026-08-06

**Where:** `userspace/oils/src/interp.rs` — the `compgen` action loop's fallthrough
arm, which carried the comment "`binding` needs a live line editor, which osh has
not got".

**What.** The comment was wrong about what the action reads. bash's
`it_init_bindings` calls `rl_funmap_names()`, which walks a table readline
*compiles in*; the names exist as soon as the library is linked, so bash answers
`compgen -A binding` in a plain script — one line after `bind -l` has warned that
line editing is not enabled — with all 174 function names, sorted. osh has had
that exact table since the `bind` work (`bind_tables::FUNCTION_NAMES`, which is
what its own `bind -l` prints); only the completion action was not reading it.

```text
$ compgen -A binding yank
bash: yank yank-last-arg yank-nth-arg yank-pop
osh : (nothing, rc=1)
```

**Fix (2026-08-06).** The action extends the candidate list from
`bind_tables::FUNCTION_NAMES`, whose order is already `bind -l`'s.

**Tests.** `tests/corpus/the-binding-completion-is-readlines-funmap-not-a-live-editor.sh`
(the two listings compared with `cmp`, the sortedness, prefix narrowing, the
invalid-action diagnostic, and the action-table ordering against `builtin`) plus
the `the_binding_action_is_readlines_funmap` unit test.

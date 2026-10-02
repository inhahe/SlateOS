### TD-OILS-COMPLETE-NOOP. `complete`/`compopt` register/print/remove completion specs but never generate completions (osh has no interactive tab-completion), and multi-spec `complete -p` lists in insertion order, not bash's hash order — MINOR, by design 2026-07-19

**Where:** `userspace/oils/src/interp.rs` — `builtin_complete` / `builtin_compopt`,
the `CompSpec`/`CompKey` types, the `comp_specs: Vec<(CompKey, CompSpec)>` field,
and the `format_compspec` renderer.

**What:** the `complete` and `compopt` builtins are implemented for *script
compatibility* — a sourced `bash_completion` file (or any script) calls
`complete …` hundreds of times, and previously each errored with `complete:
command not found` (status 127), aborting the source. Now they parse fully,
store the spec, print it re-executably (`complete -p`, byte-matching bash for a
single spec, including the fixed option print order and `'\''` quoting), mutate
it (`compopt -o`/`+o`), and remove it (`complete -r`). Two intentional
limitations remain:

1. **Specs are never *used*.** osh's REPL is line-oriented with no Readline tab
   completion, so a registered `-F func`/`-C cmd`/`-W list` generator is stored
   but never invoked to produce candidates. `compgen` (which *does* generate
   candidates on demand) is the functional half; `complete` is the registration
   half with no completion engine behind it. Note that the *generators*
   themselves are no longer the gap: since 2026-07-31 `compgen -F func` calls the
   function (binding `COMP_LINE`/`COMP_POINT`/`COMP_TYPE`/`COMP_KEY`/
   `COMP_CWORD`/`COMP_WORDS` and reading `COMPREPLY`) and `compgen -C cmd`
   substitutes the command, both with bash's "may not work as you expect"
   warning; the same day the `job`/`running`/`stopped` actions grew to read the
   job table and `hostname` to read `$HOSTFILE`, `service` to read
   `/etc/services`, and `user`/`group` to read `/etc/passwd` and `/etc/group`.
   What is missing is only the *engine* that would consult a stored spec at a
   prompt — and `binding`, which needs a live line editor and is parsed and
   ignored. (The three file-backed actions read real files on SlateOS but find
   nothing on the Windows host the differential corpus runs on, so they have
   unit tests rather than corpus cases.)
2. **`complete -p` (list all) uses insertion order**, whereas bash iterates its
   internal hash table (an order that depends on bash's string-hash + bucket
   layout and is not reproducible without replicating those internals). Each
   *individual* `complete -p NAME` line matches bash exactly; only the relative
   order of *unrelated* specs in a full listing can differ.

**Why by design:** (1) is blocked on there being an interactive line editor with
completion at all (a much larger, separate feature); until then a stored-but-unused
spec is the correct bash-compatible behavior for non-interactive scripts. (2) is a
deliberate trade — matching bash's hash-bucket order byte-for-byte has no practical
value (scripts that care read `complete -p NAME`, not the unordered full dump) and
would require hard-coding bash's hash function. Fixing (1) would also make (2)
moot in practice (real completion never depends on listing order).

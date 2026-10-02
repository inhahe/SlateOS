### TD-OILS-COMPLETE-TABLE-AND-OPERANDS. `complete`/`compopt` got ten things wrong at once, all downstream of two facts about bash they did not model — 2026-08-03 — ✅ **RESOLVED 2026-08-03**

**Where:** `userspace/oils/src/interp.rs` — `builtin_complete`, `builtin_compopt`,
`comp_key_label`, `comp_remove`, the `comp_specs` field and the subshell clone.

**The two facts.**

1. **The registry is a hash table, not a list.** bash's `pcomplib.c` builds it
   with **512 buckets** keyed by `hash_string`, which is **32-bit FNV-1**
   (offset basis 2166136261, prime 16777619, *multiply then xor*). `hash_insert`
   links a new item at the **head** of its bucket chain and `hash_walk` visits
   buckets ascending, so a whole-table `complete -p` prints in
   `(bucket asc, insertion index desc)` order — and a *redefinition* reuses the
   item already in the chain, so it does **not** move to the front.
   The byte type matters: bash dereferences a `char *`, which is **signed** on
   x86, so bytes ≥ 0x80 sign-extend. That is measurable — a spec named `\x80`
   lands in bucket 159 and prints *before* one named `aa` (bucket 315), which
   only the signed reading predicts.
2. **The option scan in front of it is plain getopt.** The first word that is
   not an option ends the options, so a flag written *after* a name is itself a
   name: `complete -W 1 aa -o nospace` registers **three** specs (`aa`, `-o`,
   `nospace`), none carrying the `nospace` option.

**The divergences that followed.**

* Listing order was insertion order. Now `Shell::comp_print_order` reproduces
  the table walk (`comp_hash_bucket` is the hash).
* `-D`/`-E`/`-I` were treated as targets appended to the name list. They are
  flags: only the **first of the three in that fixed order** takes effect
  whatever order they are written in (`complete -W 1 -I -E -D` defines the
  *default* spec), and it **replaces** the name operands rather than joining
  them — `complete -W 1 -D kk` never mentions `kk` again. `-p` and `-r` read
  the operand list the same way.
* The specials were labelled by their flag letters in diagnostics. bash stores
  them in the same table under reserved names and never translates back:
  `complete -p -D` with nothing registered really does say
  `complete: _DefaultCmD_: no completion specification`.
* `complete -r NAME` always returned 0. It answers *per name*: an absent one is
  reported to stderr and the status is 1 (a bare `complete -r`, which clears
  everything, still succeeds even on an empty table).
* `-F` was unvalidated. bash rejects an argument containing any of
  `shell_break_chars` = `( ) < > ; & | space tab newline` with
  ``complete: `ARG': not a valid identifier`` and status 2, registering
  nothing — checked *inline* in the option loop, so error precedence follows
  the written order. Despite the wording, `1bad` and `` are accepted.
* A printed name was never quoted. bash single-quotes it when it is empty or
  holds a shell metacharacter (`sh_contains_shell_metas`, the same set as
  osh's `xtrace_meta`, `#` and `~` position-sensitive) — but with **no ANSI-C
  fallback**, so a raw `\x01` goes out bare.
* **A seventh, found while re-running the probes.** `comp_specs` was documented
  as "not inherited by subshell clones (bash does not propagate compspecs to
  subshells)" and the clone started empty. That is backwards — a subshell forks
  the shell wholesale, so `complete -W 1 c8; complete -p | tr '\n' ' '` lists
  `c8` under bash. Every probe that piped `complete -p` into `sed`/`tr` printed
  nothing under osh for this reason alone.

**How the table was recovered.** Purely black-box: a brute-forcer over eight
candidate hash functions × seven table sizes × both chain directions, checked
against four independent orderings measured from bash, left exactly one fit
(FNV-1 / 512 / head-insert). The signedness was then settled by registering
names containing `\x80`, `\xffA` and `\xc3\xa9x` beside `aa`/`zz` and comparing
against both models, and the whole model verified against a 60-name sample and a
deliberately-constructed bucket collision (`c8`/`c174`).

**`compopt` had the same two problems, plus a whole missing mode.**

* Its option scan was word-by-word (`-o` and `+o` matched as *whole words*), so
  the clustered spellings bash accepts — `-onospace`, `+onospace`, `-DE`,
  `-Do nospace` — were rejected or misread, and options were still recognised
  after a name (`compopt zz -o nospace` must ask about three specs named `zz`,
  `-o` and `nospace`). Note `+` opens an option word here as readily as `-`,
  and only `o` reads which sign it arrived with: `+D` sets the flag `-D` does.
  A lone `-` or `+` is a word, so it is a name.
* `-D`/`-E`/`-I` were appended to the target list rather than taken first-of-three
  in D-E-I order, and did not replace the name operands.
* **With neither `-o` nor `+o` it is a query, not a no-op.** bash writes one
  line per target to *stdout* — `compopt +o bashdefault +o default … NAME` —
  naming every option in `COMP_O_ORDER` with `-o` when set and `+o` when not,
  so the line is a command that would restore the state. Targets are visited in
  the order written and are *not* deduplicated (a repeated name is answered
  twice), and a missing one's diagnostic lands between the lines around it. The
  target is rendered exactly as `complete -p` renders it (`-D` for the default
  spec, `comp_quote_name` for an ordinary name). osh printed nothing and
  returned 0.

**Coverage.** `tests/corpus/a-complete-is-a-hash-table-with-getopt-in-front.sh`
and `tests/corpus/a-compopt-reads-as-well-as-writes.sh` cover all of it
end-to-end; six lib tests cover the hash, the chain order, the quoting rule, the
reserved names, the `compopt` query form and its option scan.

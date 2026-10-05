### TD-OILS-THE-ALIAS-AND-COMMAND-MIRRORS-ARE-NOT-THEIR-OWN-TABLES. `BASH_ALIASES` and `BASH_CMDS` enumerate as associative arrays; bash views the alias and command tables — 2026-08-04 — ✅ FIXED 2026-08-04

**Where:** `userspace/oils/src/interp.rs` — `Shell::sync_bash_aliases` and
`Shell::sync_bash_cmds`, which rebuild the two mirrors by collecting into an
ordinary `AssocArray`.

**Reproduce:**

```sh
alias zz='echo z'; alias aa='echo a'; alias mm='echo m'; alias bb='echo b'
echo "${!BASH_ALIASES[@]}"     # bash: zz mm bb aa    osh: aa zz mm bb
hash -p /bin/x zz; hash -p /bin/y aa; hash -p /bin/z mm
echo "${!BASH_CMDS[@]}"        # bash: mm aa zz       osh: aa zz mm
```

**Why.** In bash these two are not associative arrays, they are *views* of the
alias table and the hashed-command table — separate `hash_create` calls with
their own bucket counts. So fixing
`TD-OILS-ASSOC-ITERATION-ORDER-IS-SORTED-NOT-HASHED` did not fix these: the
mirrors now walk a bash-shaped table, just not the right one, and fed in the
wrong order besides (`sync_bash_aliases` feeds `self.aliases`, a `BTreeMap`, in
*sorted* order; `sync_bash_cmds` has to sort at all because `self.cmd_hash` is a
`HashMap` and has no order to feed).

**Measured 2026-08-04.** Both tables were fitted the way `assoc.rs` was: a
brute force over (initial buckets, load factor, `>` vs `>=`, growth multiplier,
head/tail insertion, rehash direction, emit direction) required to reproduce
*every* set at once. Eleven `al0..alN` sets (N up to 600) and ten `cm0..cmN`
sets (N up to 1030) each leave a **single** surviving shape:

| table | buckets | grows at | chain comes out in |
|---|---|---|---|
| associative array | 1024 | `n >= 2048` | **reverse** of insertion |
| alias table | **64** | `n >= 128` | **insertion** order |
| hashed commands | **256** | `n >= 512` | **insertion** order |

Everything else is shared: FNV-1 over signed chars, `hash & (nbuckets - 1)`,
`nbuckets *= 4` on growth, and a rehash that walks the old buckets in index
order pushing each entry onto the head of its new chain — so a growth *reverses*
each chain.

The emit column is the only real difference, and it is now measured rather than
guessed. The `alN`/`cmN` sets could not show it — not one of them collides — so
`aft aoo atj bfs`, which share a bucket at 64 *and* at 256, were used instead:

```
alias aft=: aoo=: atj=: bfs=:   →  aft aoo atj bfs      (insertion order)
alias bfs=: atj=: aoo=: aft=:   →  bfs atj aoo aft      (so: not sorted)
… then unalias atj; alias atj=: →  aft aoo bfs atj      (a re-add goes last)
… then alias aft=x             →  aft aoo atj bfs      (a rewrite does not relink)
… then 130 more, forcing 64→256 →  bfs atj aoo aft      (a growth reverses)
```

The assoc array's own colliding set (`aaa fan jfk pkb`) comes out *reversed*
under the same treatment, so the two really do differ. Head insertion with the
chain walked from the tail is indistinguishable from tail insertion walked from
the head, and the growth line above pins which of the two the *rehash* does; the
table below is written in whichever spelling is clearer.

**This measurement already paid for itself:** the alias fit came back
`n >= nbuckets * 2` where `assoc.rs` had shipped `>`, and one hashlib cannot have
two rules. Asking bash for exactly 2049 keys showed `assoc.rs` was the wrong one.
See the "Corrected 2026-08-04" note under
`TD-OILS-ASSOC-ITERATION-ORDER-IS-SORTED-NOT-HASHED`.

**One insertion rule, two readers.** The "chain comes out in" column above is a
property of the *mirror*, not of the table, and that turned out to be the whole
shape of the fix. Measuring `hash -l` against `${!BASH_CMDS[@]}` over the same
four-name table shows them disagreeing:

```
hash -p /p/aft aft; … /p/aoo aoo; … /p/atj atj; … /p/bfs bfs
hash -l          →  bfs atj aoo aft      (walks each chain head→tail)
${!BASH_CMDS[@]} →  aft aoo atj bfs      (the mirror comes out the other way)
alias -p         →  sorted, ignoring the table's order entirely
```

So there is no per-table insertion direction to model. All three tables insert
at the head; `hash`/`hash -l` read a chain forwards, the two mirrors read it
backwards, and `alias -p` sorts. That collapses part 2 of the plan below into a
five-line `mirrored()` view.

**Fixed 2026-08-04.**

- `src/assoc.rs`: `AssocArray` is now generic in its value (`AssocArray<V = Str>`)
  and carries a `TableShape` (`Assoc` → 1024 buckets, `Alias` → 64,
  `Command` → 256), constructed with `with_shape`. Everything else — FNV-1 over
  signed chars, `hash & (nbuckets - 1)`, head insertion, `nbuckets *= 4` at
  `n >= nbuckets * 2`, chain-reversing rehash — is shared, because bash's
  `hashlib.c` is shared.
- `AssocArray::mirrored(f)` builds the view: same shape, same buckets, each chain
  reversed, each value mapped. `sync_bash_aliases` mirrors with `Clone::clone`;
  `sync_bash_cmds` mirrors with `|(p, _)| path_to_bytes(p)`, dropping the hit
  count.
- `src/interp.rs`: `aliases` is an `AssocArray` of shape `Alias` (was
  `BTreeMap<Str, Str>`) and `cmd_hash` an `AssocArray<(PathBuf, u64)>` of shape
  `Command` (was `HashMap`). `unalias -a` calls `reset()` (bash's
  `delete_all_aliases` disposes the table); `hash -r` and the PATH-change hook
  call `clear()` (bash's `hash_flush` keeps the buckets).
- `hash` / `hash -l` lost their sort and `alias -p` gained one — the two
  builtins genuinely differ, and dropping the `BTreeMap` would otherwise have
  silently unsorted `alias -p`.
- `src/parser.rs` and `src/lexer.rs` take `&AssocArray` in place of
  `&BTreeMap<Str, Str>`. The parser's alias-staleness check relies on
  `AssocArray`'s derived `PartialEq` being *structural* — order-sensitive —
  which is what that check wants.

**Pinned by** `tests/corpus/the-alias-and-command-tables-are-bashs-own.sh`, which
uses `aft aoo atj bfs` (one bucket at 64 *and* at 256) to show chain order rather
than bucket order, and covers: three insertion orders, a removal, a re-add, a
rewrite, `unalias -a`, a 64→256 growth, the same for the command table, the
`hash` / mirror disagreement, `hash -r` and `hash -d`, the same four keys in an
ordinary associative array (a *third* order, `aft bfs aoo atj`), subshell
inheritance, and defining an alias by writing through the mirror. Bucket counts
and growth points are pinned separately by the unit tests in `src/assoc.rs`.

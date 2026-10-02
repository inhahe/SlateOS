### TD-OILS-ASSOC-ITERATION-ORDER-IS-SORTED-NOT-HASHED. an associative array iterates in sorted key order; bash iterates its hash — 2026-08-04 — ✅ **FIXED 2026-08-04**

**Where:** `userspace/oils/src/assoc.rs` — the `AssocArray` the `assoc` map
holds, and so every reader that walks it (`Shell::array_elements`,
`Shell::slice_elements`, the `${!m[@]}` key list, `declare -p`, the `${m[@]}`
value list, `for k in "${!m[@]}"`).

**Reproduce:**

```sh
declare -A m=([k1]=v1 [k2]=v2 [k3]=v3 [k4]=v4)
echo ${!m[@]}   # bash: k4 k1 k2 k3      osh: k1 k2 k3 k4
echo ${m[@]}    # bash: v4 v1 v2 v3      osh: v1 v2 v3 v4
```

osh keeps the map ordered by key, so its output is sorted and stable. bash walks
its own open-hash table, so the order is a function of the hash of each key, the
table size, and the insertion history — `k4` coming first here is not an
accident of this run, it is what bash's string hash and its 64-bucket initial
table produce for these four keys.

**Why it matters.** It blocks *any* corpus case that prints more than one
element of an associative array in a context where the order is observable —
which is nearly all of them. Existing cases work around it by using a single
key, by sorting the output, or by printing counts instead of values. It is also
why `TD-OILS-ASSOC-SLICE-OFFSET-IS-ONE-BASED` cannot be pinned by a
straightforward multi-key fixture.

**The fix.** `userspace/oils/src/assoc.rs` is now a port of bash's `hashlib.c`:
`AssocArray` is the bucket array itself (`Vec<Vec<(Str, Str)>>`, each chain from
its head) rather than an insertion-ordered `Vec` with a hash index beside it, so
`iter`/`keys`/`values` are the same walk bash makes and every reader inherits the
order for free. Lookup stays O(1) — the load factor bounds a chain at a handful
of entries — which was the property the old shape existed to provide.

**The parameters were measured, not read out of a header.** They are the only
combination that fits bash 5.2.37 over the key sets `key0..key{n}` for n of 4, 8,
16, 32, 40, 64, 100, 200, 300, 600, 2050 and 4200 (crossing a growth), plus short
sets, numeric-looking keys, high-byte keys, removals, re-adds and `m=()` resets:

| | |
|---|---|
| hash | **FNV-1** — multiply *then* xor — 32-bit, seed `2166136261`, prime `16777619` |
| byte | xored in as a **signed** char, so `\xff` contributes `0xffffffff` (bash walks a `char *`) |
| bucket | `hash & (nbuckets - 1)`, starting at **1024** buckets |
| insert | at the **head** of the chain, so within a bucket the order is the reverse of insertion |
| growth | `nentries >= nbuckets * 2` → `nbuckets *= 4`, checked *before* the entry is linked |
| rehash | old buckets in index order, each entry pushed onto the **head** of its new chain |
| removal | unlinks only; the table never shrinks and nothing else moves |

**What it moved.** Six unit tests in `interp.rs` asserted the old order
(`assoc_all_values_and_keys`, `assoc_literal_elements_are_not_split_or_brace_expanded`,
`assoc_quoted_all_preserves_fields`, `declare_p_quotes_associative_keys_like_bash`,
`export_assigns_through_the_array_aware_store`, `param_transform_keyvalue`);
each new expectation was re-derived from bash's own output for the same script
rather than from the new implementation, and all six now match it byte for byte.

**Corrected 2026-08-04:** the growth test shipped as `>` and is `>=`, so osh
grew a 1024-bucket table on its 2050th key where bash grows on its 2049th. It
took a second table to find: the same model fitted to the *alias* table (see
`TD-OILS-THE-ALIAS-AND-COMMAND-MIRRORS-ARE-NOT-THEIR-OWN-TABLES`) came back
`>=`, which cannot be right for one hashlib and wrong for the other, and asking
bash for exactly 2049 keys settled it. The reason nothing caught it: past a
growth most chains hold a single entry, and a lone entry lands the same way
whether it was rehashed into its bucket or inserted there — 2050, 2100 and 4200
keys all come out *identical* under either rule. `assoc.rs` now pins 2048 and
2049 in `grows_on_the_key_that_reaches_the_load_factor`, the only nearby sizes
that tell the two apart.

**Pinned by** `userspace/oils/tests/corpus/an-associative-array-iterates-in-bashs-hash-order.sh`,
which covers the bucket walk, the chain reversal (`aaa fan jfk pkb` collide, so
inserting them backwards reverses them, while eight non-colliding keys are
insensitive to their insertion order), a growth at 2100 keys, removal and re-add,
a rewrite that does not relink, `m=()` starting a fresh table, high-byte keys,
and the six readers agreeing (`${!m[@]}`, `${m[@]}`, `declare -p`, `@K`, `for`,
and a slice).

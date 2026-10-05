### TD-OILS-ASSOC-ORDER. `osh` iterates associative arrays in insertion/sorted order, not bash's hash order — 2026-07-19

**Where:** `userspace/oils/src/interp.rs` associative storage (`self.assoc`
is an insertion-ordered map) and every `[@]`/`[*]`/`${!m[@]}` expansion
that reads it (`array_elements`, `array_keys`, `bulk_elements`, …).

**What:** `declare -A m=([x]=1 [y]=2); echo "${m[@]}"` prints `1 2` in osh
but `2 1` in bash. bash stores associative arrays in an internal hash
table and iterates in hash-bucket order (a function of its string-hash of
the keys); osh iterates in insertion order. Every associative `[@]`/`[*]`
expansion (values, keys, `${m[@]:-…}`, `${m[@]#pat}`, `for k in
"${!m[@]}"`, etc.) can therefore differ in element *order* — the contents
are identical, only the sequence differs.

**Why deferred:** reproducing bash's exact iteration order would require
reimplementing bash's specific string-hash and open-addressing bucket
walk, and scripts are not supposed to rely on associative-array order
anyway (POSIX/bash both document it as unspecified). Insertion order is
arguably more useful and is self-consistent.

**Proper fix:** only if a real script needs bash-identical ordering —
port bash's `hash.c` hash function and bucket iteration for `self.assoc`.
Not worth it absent a concrete need.

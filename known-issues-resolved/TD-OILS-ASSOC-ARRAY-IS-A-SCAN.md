### TD-OILS-ASSOC-ARRAY-IS-A-SCAN. an associative array's lookup was linear, so filling one was quadratic — ✅ **FIXED** — 2026-08-02

**Where:** `userspace/oils/src/interp.rs` — `Shell::assoc`, `assoc_set`,
`assoc_element`; now `userspace/oils/src/assoc.rs`.

An associative array was a `Vec<(Str, Str)>`, kept in insertion order so that
`${!m[@]}` and `declare -p` enumerate deterministically. Order was the only
thing it provided: every element read *and* every element write scanned it, so
a loop that fills an array is O(n²).

```sh
declare -A m; for ((j = 0; j < N; j++)); do m[k$j]=1; done
```

| N | before | after | bash 5.2.37 |
|---|---|---|---|
| 5 000 | 127 ms | 108 ms | 103 ms |
| 10 000 | 288 ms | 154 ms | 107 ms |
| 20 000 | 632 ms | 225 ms | 177 ms |
| 40 000 | 2 348 ms | 463 ms | 236 ms |

The curve was bending upward where bash's is straight; it is now straight too,
and 40 000 keys cost 5× less. The fix is a small `AssocArray` type owning both
representations — the `Vec` for the order and a `HashMap<Str, usize>` index
beside it — so the invariant that ties them together lives in one place rather
than at each of the call sites that used to mutate the `Vec` directly. `unset
m[key]` shifts every later position and so rebuilds the index; it is rare, and
paying O(n) there is what keeps everything else O(1).

It lives in its own module rather than in `interp.rs` for the same reason
`bind_tables.rs` does — see TD-OILS-LIB-TEST-BUILD-RUNS-OUT-OF-MEMORY.

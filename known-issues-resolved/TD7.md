### TD7. `set_mempolicy_home_node` returns 0 where Linux returns `-ENOENT`/`-EOPNOTSUPP` — APPROXIMATION 2026-06-12

**What:** `sys_set_mempolicy_home_node()` (`kernel/src/syscall/linux.rs`)
returns 0 for any valid non-empty range. Linux v6.6 instead walks the VMAs
in `[start, end)` with `err` initialized to `-ENOENT`: it returns `-ENOENT`
when no VMA in the range carries an explicit `MPOL_BIND`/`MPOL_PREFERRED_MANY`
policy, and `-EOPNOTSUPP` for a VMA whose policy is some other mode. Only a
range that already has a bind/preferred-many policy yields 0.

**Why we diverge:** our `mbind` is a UMA no-op that does **not** store
per-VMA mempolicy, so the kernel cannot tell whether the caller previously
established a policy on the range. We pick 0 (the "policy was set, home node
applied" success outcome — the common real-world sequence where
`set_mempolicy_home_node` follows a successful `mbind(MPOL_BIND)`) over
`-ENOENT`. Returning `-ENOENT` would instead break that common path.

**Proper fix:** implement real per-VMA mempolicy storage so the VMA walk can
distinguish "no policy" (`-ENOENT`), "wrong policy" (`-EOPNOTSUPP`), and
"bind policy → apply home node" (0). Tracked as an open question
(`open-questions.md`) because the 0-vs-`-ENOENT` choice is a genuine
tradeoff. **Note:** batch 551 *did* fix the unambiguous part — the
`home_node` online check now runs before the len/end gates, matching v6.6.

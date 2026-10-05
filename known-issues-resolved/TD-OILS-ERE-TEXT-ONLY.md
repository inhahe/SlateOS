### TD-OILS-ERE-TEXT-ONLY. `[[ … =~ … ]]` cannot match a non-text subject or pattern — 2026-07-30 — ✅ RESOLVED 2026-07-30

**Where:** `userspace/oils/src/interp.rs` — `cond_regex` (~`:6285`), and the
whole of `userspace/oils/src/ere.rs`, which is `&str`-typed
(`Regex::new_flags(&str)`, `captures(&str) -> Vec<Option<String>>`).

**What:** with the value layer byte-typed, a subject or pattern can now contain
a byte that is part of no character. `cond_regex` routes both through
`bytes::as_str` and, on `None`, gives the answer the engine itself would give
for input it cannot represent: an unrepresentable **pattern** is an uncompilable
RHS (`cond_regex_error`, `[[` exits 2, matching bash's behaviour for a bad
regex), and an unrepresentable **subject** matches nothing. So the failure is
honest and conservative — it never matches a *different* string than the user
has, which is what a lossy widening here would do. But it is still a functional
gap: `[[ $f =~ ^a ]]` is false for a filename with a stray `\xff` in it, where
bash in the C locale matches.

`BASH_REMATCH` is already `BTreeMap<usize, Str>`, so the store side needs no
change; only the engine does.

**Fix:** convert `ere.rs` to match over `&[u8]` with `bytes::Ch` for the
character-defined constructs (`.`, bracket expressions, POSIX classes,
case-folding under `nocasematch`) — the same treatment the glob engine already
got in TD-OILS-BYTE-STRINGS step 5, and directly modelled on it. `.` must match
one `Ch`, so it matches an undecodable byte as a unit rather than a third of a
character. Scheduled as TD-OILS-BYTE-STRINGS step 9.

**Resolved 2026-07-30**, exactly as sketched: `ere.rs` now scans `Ch`, both
`cond_regex` seams were deleted, and `EreError` carries `Str` with no `Display`
(bash prints nothing for an uncompilable `=~` RHS, so nobody rendered it).
`[[ $f =~ ^a ]]` is now true for `f=$'a\xffb'`, `.` matches that byte as one
character, `[^a-z]` matches it while `[a-z]` and `[[:alpha:]]` do not, and the
capture that reaches `BASH_REMATCH` is the byte itself. Pinned by
`ere::tests::matches_a_subject_and_a_pattern_that_are_not_text` and
`interp::tests::cond_regex_matches_a_value_that_is_not_text`. See
TD-OILS-BYTE-STRINGS step 9 for the full write-up.

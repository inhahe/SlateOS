### BUG-OILS-ASSOC-ARRAY-SEMANTICS. Associative arrays diverged from bash in four independent ways — 2026-07-27 — ✅ RESOLVED 2026-07-27

**Symptom.** Four separate defects, all found by writing the `assoc-arrays.sh`
differential corpus case:

1. **Silent data loss on a kind conflict.** `declare -A name` on an existing
   *indexed* array (and `declare -a` on an associative one) silently created an
   empty map of the new kind. Every subsequent read went to the new, empty
   table while the original values sat unreachable in the other one — the array
   looked like it had been erased. bash refuses the conversion outright.
   ```sh
   declare -a a=(x y); declare -A a; echo "${#a[@]}"   # osh: 0, bash: 2 + error
   ```
2. **`m=(k1 v1 k2 v2)` rejected outright.** osh required a subscript on *every*
   element of an associative literal, so the extremely common pairwise idiom
   errored on each word and stored nothing. bash picks one of two modes from
   the *first* element: unsubscripted-first consumes the whole list as
   alternating key/value words (a later `[k]=v` then becomes the literal key
   `[k]=v`), subscripted-first requires subscripts throughout.
3. **Empty subscripts silently accepted.** `m['']=x` created a key that no
   expansion can name, and `${m['']}` read it back. bash rejects both.
4. **Wrong message shape and flow for the rejections.** The offending word in
   `must use subscript when assigning associative array` is quoted only for a
   `declare`/`local` operand, not for a bare `m=(…)`; and the kind-conflict
   message carries the `declare:` tag only in the bare-name form, where the
   compound-assignment form is untagged *and* discards the rest of the parse
   unit (status 1) rather than merely failing.

**Root cause.** `userspace/oils/src/interp.rs`. (1) `exec_declare_with_arrays`
and `builtin_declare` created the entry with `entry().or_default()` without ever
consulting the other table. (2) The `AssignRhs::Array(items) if is_assoc` arm
had a single always-error `ArrayElem::Positional(_)` branch. (3) The assoc
branches of `apply_assignment` and `expand_array_ref` used the expanded key
without an emptiness check.

**Fix.** (1) An `array_kind_conflict` helper called from both declare paths,
*after* `declare_local` so `local -A` shadowing a global of the other kind stays
legal. (2) The assoc-literal arm now selects pair mode from
`matches!(items.first(), Some(ArrayElem::Positional(_)))`, flattening keyed
elements back to their `[idx]=val` source text so they land as literal keys.
(3) Emptiness checks on both the write and read paths, reporting the source form
of the subscript via `crate::unparse::word_src`. (4) A `decl_array_ctx` flag
saved/restored around the inner `apply_assignment`, and `Flow::Discard` from the
literal-path kind conflict.

**Found by** the `assoc-arrays.sh` differential corpus case; covered by the
`declare_cannot_convert_between_array_kinds`,
`assoc_compound_assignment_has_two_modes` and
`assoc_empty_subscript_is_rejected` unit tests.

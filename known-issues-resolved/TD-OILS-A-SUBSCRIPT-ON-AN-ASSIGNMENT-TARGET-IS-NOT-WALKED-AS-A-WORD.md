### TD-OILS-A-SUBSCRIPT-ON-AN-ASSIGNMENT-TARGET-IS-NOT-WALKED-AS-A-WORD. `a[$(( #5 ))]=1` named only the `$((…))` and, for an associative array, reported a second time — 2026-08-04 — ✅ FIXED 2026-08-05

**Where:** `userspace/oils/src/interp.rs` — `Shell::assignment_scan_failed`,
called from `Shell::apply_assignment` and `Shell::prefix_assignment_name`.

**What:** a subscript is not a word in its own right — it is arithmetic read
out of the assignment — so `Shell::begin_word` never saw it and the fault
surfaced only later, from the evaluation-time backstop in `Shell::arith_sub`.
Two deviations followed. The naming was wrong, and worse, the assignment then
carried on with an empty subscript and complained a **second** time about the
"bad array subscript" its own first error had produced:

```text
$ declare -A m; m[$(( #5 ))]=1
bash: bad substitution: no closing `)' in m[$(( #5 ))]=1
osh : bad substitution: no closing `)' in $(( #5 ))
osh : m[$(( #5 ))]: bad array subscript          <- bash emits no such line
```

**Fixed** by scanning the assignment's subscripts before performing it. bash
names the assignment for a subscript fault and the *value word* for a value
fault, and both spellings now agree exactly — measured across seven forms:

```text
a[$(( #5 ))]=1              -> a[$(( #5 ))]=1
a[$(( #5 ))]=$(( 1 ))       -> a[$(( #5 ))]=$(( 1 ))
declare a[$(( #5 ))]=1      -> a[$(( #5 ))]=1
a=([$(( #5 ))]=1)           -> [$(( #5 ))]=1        (the element, not the literal)
x=1 a[$(( #5 ))]=1 true     -> a[$(( #5 ))]=1       (before "not a valid identifier")
a[1]=$(( #5 ))              -> $(( #5 ))            (the value is a word)
unset "a[$(( #5 ))]"        -> "a[$(( #5 ))]"       (already right: an operand word)
```

Pinned by `an_assignment_subscript_is_scanned_as_part_of_the_assignment` and
three new sections in the corpus case.

**Standing lesson:** the second diagnostic was the real bug, and it was the
easier one to miss — it looked like extra helpfulness. A message about the
*consequence* of a failure already reported tells the reader about the shell's
control flow rather than about their script. When an error arms a discard, the
code downstream of it should be looking for a way to stop, not for something
else to say.

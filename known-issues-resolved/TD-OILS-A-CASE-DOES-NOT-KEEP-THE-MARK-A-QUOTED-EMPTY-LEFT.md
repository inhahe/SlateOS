### TD-OILS-A-CASE-DOES-NOT-KEEP-THE-MARK-A-QUOTED-EMPTY-LEFT. `case abc in ${x:-a''bc})` matches where bash does not — 2026-08-04 — ✅ FIXED 2026-08-04

**Where:** `userspace/oils/src/interp.rs` — `Shell::exec_case`, and the two
expansions it uses. bash's `case` is the **one** construct that does not do
quote removal on either side: it expands with `expand_word_leave_quoted` and
matches the `CTLNUL` bytes as they stand, so the mark that
`TD-OILS-QUOTED-EMPTY-IN-AN-OPERAND-LEAVES-NO-FIELD` introduced is a real
character there. osh loses it twice over, and differently on each side:

* the **pattern** goes through `Shell::expand_cond_pattern`, which does make
  marks and then drops them in `Shell::operand_chars`;
* the **subject** goes through `Shell::expand_cond_string`, which reaches
  `Shell::expand_word_joined` — a path that works in `Str` and never makes an
  `EChar` at all, so there is no mark to keep.

**Reproduce:**

```sh
case abc in ${nope:-a''bc}) echo match;; *) echo no;; esac   # bash: no   osh: match
case abc in ${nope:-''a*})  echo match;; *) echo no;; esac   # bash: no   osh: match
case ${nope:-a''bc} in abc) echo match;; *) echo no;; esac   # bash: no   osh: match
```

**The rule** — measured on both sides at once. The mark is the byte `\177`
(bash's `CTLNUL`), a character like any other:

| subject | pattern | bash |
|---|---|---|
| `a`▫`bc` | `abc` | no |
| `a`▫`bc` | `a?bc` | match — `?` absorbs it |
| `a`▫`b` | `a*b` | match |
| `a\177b` (real) | `a`▫`b` | **match** — the mark *is* that byte |
| `abc` | `abc`▫ | no |

with exactly one exception, on **both** sides: a word that is *nothing but a
single mark* is the empty string instead. That is bash's `QUOTED_NULL` macro,
applied to the pattern by `quote_string_for_globbing(…, QGLOB_CVTNULL)` and to
the subject by the same test.

| subject | pattern | bash |
|---|---|---|
| `` (a literal `''`) | ▫ | match |
| `` | ▫▫ | no |
| `\177` | ▫ | no |
| ▫ (from `${x:-''}`) | ▫ | match — both are `""` |
| ▫ | ▫▫ | no |
| ▫▫ | `??` | match |
| ▫▫ | `?` | no |

Every other construct removes it, and osh is already right about those:
`[[ abc == ${nope:-''a*''} ]]` matches, `${w#${nope:-''a''}}` strips one
character, `v=${nope:-''x''}` assigns `x`, and `${nope:-''g/a*}` globs `g/a*`.

**Only an operand leaves one.** A quoted empty written in the `case` word
itself is transparent on both sides — `case abc in ''a''b''c'')` matches, and so
does `case a''bc in abc)`, and `"${f[@]}"` with one empty element is transparent
too. That is what osh already does (marks are made only in
`SplitMode::Operand`), so the fix is entirely about not *dropping* them.

**The fix.** `keep_marks` is the `case` half of quote removal: it maps
`EChar::MARK` to the byte `\177` rather than to nothing, and applies the
`QUOTED_NULL` exception once to the word it is given. Both sides go through it,
each from an entry of its own — `Shell::expand_case_pattern` and
`Shell::expand_case_subject` — and every other caller keeps `drop_marks`, which
moved out of `Shell::operand_chars` (which now hands its marks over intact) and
into the three consumers that are quote removal: `Shell::expand_word_pattern`,
the `SplitMode::Text` arm of `Shell::expand_word_annotated`, and
`Shell::expand_word_joined`.

The subject needed the refactor the plan called for: `Shell::expand_word_joined`
built a `Str` directly and so never made an `EChar` at all. It is now the
quote-removed view (`echars_text`) of a new
`Shell::expand_word_joined_annotated`, whose one changed arm asks
`Shell::operand_chars` before falling back to `Shell::joined_value` — the same
order `Shell::expand_word_pattern_inner` already used.

**Found:** while fixing `TD-OILS-QUOTED-EMPTY-IN-AN-OPERAND-LEAVES-NO-FIELD`;
it is the same mark seen from the one construct that keeps it.

**Pinned by** `userspace/oils/tests/corpus/a-case-is-the-one-match-that-keeps-the-mark.sh`.

**Left behind at the time:** a quoted `[@]` list *inside* the operand takes the
other marks with it — `TD-OILS-A-LIST-IN-AN-OPERAND-TAKES-THE-OTHER-MARKS-WITH-IT`
below, since fixed.

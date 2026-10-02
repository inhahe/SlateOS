### TD-OILS-A-DEFERRED-BRACE-BODY-KEEPS-ITS-SUBSTITUTION-AS-WRITTEN. `${#x:-$( (echo a) )}` should come back re-printed — 2026-08-07 — ✅ FIXED 2026-08-08

**Fixed** in `ca8b731a5`, text-only, once the recorded blocker turned out not to
be one (see below). `defers_its_body` became `deferred_body_mut`, returning the
body text rather than a bool, and the re-prints `parse_arith_comsubs` had always
computed are now spliced into it instead of dropped. `parse_braced_param_in`
still receives the *unspliced* `raw`, deliberately, which is what keeps the line
accounting on the source. Corpus case:
`tests/corpus/a-deferred-brace-body-carries-the-reprint-but-not-its-lines.sh`,
which pins the re-printed text, the multi-line diagnostic, the source-based
lines, and a body that builds operand words as a control (it agrees by a
different path, so the fix cannot look right for the wrong reason). 1360 unit
tests, clippy clean, and the 103 brace/param/transform/arith/declare/subst/quote
corpus cases all pass.

**Where:** `userspace/oils/src/parser.rs` — `seg_to_part`'s `Seg::ParamBraced`
arm, which ran `parse_arith_comsubs` for its side effect and dropped the
re-prints it returns.

**What.** A `${ … }` body is read by `parse_matched_pair` under `P_DOLBRACE`,
which sends a nested `$(` to `parse_comsub` (parse.y:3929 → 3959), so the body
text that travels downstream holds the re-print. osh agrees on every path that
builds operand *words* from the body — those substitutions become real AST
nodes that `unparse` re-prints. It diverges on the paths that keep the body as
unparsed text (`WordPart::BadSubst`, `BadTransform`), where the source survives:

```text
b() { : ${#x:-$( (echo 2) )}; }; declare -f b
  bash: : ${#x:-$( ( echo 2 ))}
  osh:  : ${#x:-$( (echo 2) )}
```

**Proper fix — the recorded obstacle is not one; measured 2026-08-07.** The note
below used to say that splicing moves every offset into the body, that
`frag_line` derives a fragment's physical line from exactly those offsets, and
so the line map had to be rebuilt against the spliced text in the same change.
Measured against bash 5.2.37, that last step is wrong: bash's line accounting
inside a `${ … }` body follows the **source**, not the spliced re-print.

```sh
echo one
unset x
echo "${x:-$(if true; then echo a; fi)
`fi`}"
```

The backtick is on source line 4, and its body is on the brace body's *second*
line, after a substitution that re-prints to three lines where the source had
one. If the numbering followed the spliced text the failure would be blamed to
line 6. bash says `line 4`, and says the same with a plain `$(echo a)` in place
of the `if` — i.e. the extra re-printed lines do not shift it at all.

So the body travels downstream in two coordinate systems at once, and bash keeps
them apart: the **text** carries the re-print (which is what the deparse above
shows), while the **lines** stay on the source. The fix is therefore text-only —
splice for the paths that keep the body as unparsed text (`WordPart::BadSubst`,
`BadTransform`, i.e. `defers_its_body`), and deliberately do *not* let the splice
reach `frag_line`/`map_frag_segs`. That is a much smaller change than the
original note implies, and it means this entry was never blocked on the same
work as TD-OILS-A-SPLICED-REPRINT-DOES-NOT-MOVE-LINENO (whose own recorded fix
also turned out to rest on a false premise — there was no splice there at all).

Still to check before writing it: whether any *other* consumer of the deferred
body's text derives a position from it, and what happens when the spliced body
is re-lexed (a re-print can introduce newlines inside what was one line, so a
here-document delimiter or a comment in the body needs a probe).

**Found by** the probe matrix for
TD-OILS-AN-ARITHMETIC-STRING-NAMES-ITS-COMMAND-SUBSTITUTION-AS-WRITTEN,
2026-08-07.

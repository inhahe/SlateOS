### `TD-COREUTILS-GREP-DOES-NOT-SUPPRESS-ON-ENCODING-ERRORS` -- blocked on locale support

**In short:** real grep calls a file binary for two separate reasons -- it
holds a NUL, or its bytes are not valid text in your language setting. We now
implement the first. The second cannot be implemented yet because coreutils has
no concept of a language setting at all.

This is a genuinely independent mechanism, not a corner of the NUL rule, and
the difference was established by measurement (`bin4.sh`) after a first probe
gave the opposite answer -- `dash`'s `printf` has no `\x` escape, so a `\xff`
fixture had silently written four literal characters and the file was plain
ASCII. The byte must be written octal, `\377`.

What GNU does with `printf 'ary a\377b\nary plain\n'` under `C.UTF-8`:

| command | result |
|---|---|
| `grep ary enc` | prints only `ary plain`; the bad line is dropped and `grep: enc: binary file matches` goes to stderr |
| `grep -c ary enc` | `2` -- the count is unaffected |
| `grep -o ary enc` | prints both `ary`s and no diagnostic: the printed *segment* is valid even though its line is not |
| `grep -I ary enc` | same as plain `grep` -- **`-I` does not suppress on encoding errors** |
| `LC_ALL=C grep ary enc` | prints both lines, no diagnostic: in a single-byte locale no byte sequence is invalid |

Upstream's mechanism is `print_line_head` dropping the segment it was about to
print when it has encoding errors and setting `encoding_error_output`, which
`finish_grep` then reports with the same message. So it is per printed segment,
decided at print time -- structurally elsewhere from the per-buffer NUL scan
that happens before searching.

**The blocker is a real prerequisite, not effort.** `grep -rn 'LC_ALL|LC_CTYPE|
fn locale' userspace/coreutils/src` finds nothing but doc comments: coreutils
has no locale plumbing whatsoever. Hardcoding UTF-8 validation would be wrong
under `LC_ALL=C`, where the correct answer is that *no* byte sequence is
invalid and the whole mechanism is off -- so implementing this before there is
something to ask "which locale?" would make the common `LC_ALL=C` case worse,
not better. Revisit when coreutils grows locale support; the measurements above
are the acceptance criteria and need no re-taking.

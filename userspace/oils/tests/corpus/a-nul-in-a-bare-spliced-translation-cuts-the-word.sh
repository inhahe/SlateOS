# A `$'…'` may translate to a NUL, and a NUL cannot be carried in a C string.
# Where bash loses it — and how much it loses with it — depends on which of the
# two paths the translation takes out of `parse.y`.
#
# The ordinary paths splice a C string, so the NUL ends the *translation* right
# there and the rest of it is dropped at the splice — the word itself is
# untouched and runs on. That is what a plain `$'…'` does, and what the first
# two rows of `an-ansi-c-string-in-a-double-quoted-brace-body-is-spliced-bare.sh`
# do through `sh_single_quote (ttrans)`. The token buffer never sees a NUL.
#
# The third row does not re-quote. A `$'…'` written **bare** inside a
# double-quoted `${ … }` body is spliced in as bytes:
#
#     nestret = ansiexpand (…, &ttranslen);   /* parse.y:3890 */
#     …
#     nestlen = ttranslen;                    /* parse.y:3892 */
#
# — a pointer *and a length*, so the NUL survives into the token buffer. It is
# then the **word** that ends at it, because `make_word` copies the buffer with
# `savestring`, and `strcpy` stops at the first NUL. Everything the token
# accumulated after the NUL is gone, including the `}` and the closing `"` the
# script wrote.
#
# What is *not* lost is the word's extent. The reader had already found the end
# of the word by scanning, and it goes on from there; only the text kept for the
# word is short. So the following words of the command, and the following
# commands, are read exactly as they would have been — the cut is invisible
# until something reads the word's text back.
#
# Three things read it back, and each reports the truncation in its own voice:
#
#   * `parameter_brace_expand`, at expansion time, on a `${` the cut left open:
#     ``bad substitution: no closing `}'`` naming the word *as cut*.
#   * `xparse_dolparen`, when the cut fell inside a `$( … )` body, whose
#     re-print is then unclosed — see
#     `a-cmdsub-body-is-parsed-twice-and-the-second-parse-reads-the-reprint.sh`.
#   * `parse_string_to_word_list`, when the cut word is an element of a compound
#     array assignment — see
#     `a-compound-array-assignments-value-list-is-re-read-as-a-word-list.sh`.
#
# A cut that leaves the word's text *well formed* — a `}` in the same
# translation, closing the expansion before the NUL arrives — is deliberately not
# exercised here: `known-issues.md`
# TD-OILS-A-CUT-WORD-IS-EXPANDED-AS-LITERAL-TEXT.
#
# Verified against bash 5.2.37.

e() { ( eval "$1" ) 2>&1; echo "  rc=$?"; }

unset x y

echo "=== the word ends at the NUL, and the brace the cut left open is named"
e 'echo "${x:-$'\''a\0b'\''}"'
e 'echo A"${x:-$'\''a\0b'\''}"B'
e 'echo "${x:-$'\''\0'\''}"'
# A NUL that is the whole translation cuts just as hard as one with text after.
e 'echo "${x:-$'\''\x00b'\''}"'
# The rest of the *command* is read normally — it is only this word that lost
# its tail, and the word that names the failure is the cut one.
e 'echo "${x:-ok}" "${x:-$'\''a\0b'\''}"'
e 'echo "${x:-$'\''a\0b'\''}" tail'

echo "=== the re-quoted rows lose it at the splice instead, and survive"
q=zabz
# Row two: `#` has moved `dolbrace_state` to `DOLBRACE_QUOTE`, so the
# translation goes back through `sh_single_quote` as the `z` it starts with,
# and the `}` the script wrote is still there to close the expansion.
e 'echo "${q#$'\''z\0z'\''}"'
# Row one: no `P_DQUOTE` at all, same re-quote, same survival.
e 'echo ${x:-$'\''a\0b'\''}'

echo "=== a plain \$'…' loses only the translation, and the word runs on"
# Outside a `${ … }` body the translation is appended as a C string, so it ends
# at the NUL and the *word* does not: the `b` is gone, the `y` after it is not.
e 'printf "[%s]" $'\''a\0b'\''; echo'
e 'printf "[%s]" x$'\''a\0b'\''y; echo'

echo "=== the extent is unchanged: the printback shows the cut, not a short line"
f() { echo "${x:-$'a\0b'}" second third; }
declare -f f
g() { echo one; echo "${x:-$'a\0b'}"; echo three; }
declare -f g

echo "=== the failure is the expansion's, so it ends the shell either way"
e 'echo "${x:-$'\''a\0b'\''}"; echo after'
e 'set -e; echo "${x:-$'\''a\0b'\''}"; echo after'
# It is raised where the word is expanded, not where it is read, so a word that
# is never expanded never fails.
e 'false && echo "${x:-$'\''a\0b'\''}"; echo after'
e 'f() { echo "${x:-$'\''a\0b'\''}"; }; echo after'

echo done

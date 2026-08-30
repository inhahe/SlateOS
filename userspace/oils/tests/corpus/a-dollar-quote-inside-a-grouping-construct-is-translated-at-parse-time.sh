# `$'…'` and `$"…"` are not carried into a grouping construct as text. bash's
# `parse_matched_pair` translates each one where it reads it and splices the
# result back over the `$'` / `$"` it had already written (`retind -= 2`,
# parse.y:3893 and 3921), so by the time anything downstream — an arithmetic
# evaluator, a re-lex of a `$( … )` body — looks at the text, they are gone.
#
# The arm is the `open != close` one (parse.y:3841), so it covers every
# *grouping* construct — `$( … )`, `$(( … ))`, `$[ … ]`, `<( … )` — and not
# `` ` … ` `` or `" … "`, whose delimiters match.
#
# What is spliced in is re-quoted, and which quotes depends on `P_DQUOTE`, which
# no grouping construct's own scan carries:
#
#   $'…'   ansiexpand, then sh_single_quote  (parse.y:3858, 3884)  ->  '…'
#   $"…"   locale_expand, then sh_mkdoublequoted (3901, 3919)      ->  "…"
#
# So the two land in opposite places. A single-quoted run is never valid
# arithmetic, so a `$'…'` in an expression is always an error — but the error
# names `'5'`, not `$'5'`, which is how the mechanism is visible at all. A
# double-quoted one *is* valid: it is rescanned, so `a=5; echo $(( $"a" ))`
# prints 5 and `$(( $"1+2" ))` is 3.
#
# Two further consequences of translating early:
#
#   * the ANSI-C escapes are resolved before anything else could resolve them
#     differently, and a NUL ends the string, so `$'a\0b'` becomes `'a'`;
#   * the text changes *shape*. A `$'x\ny'` in a `$( … )` body puts a real
#     newline where the source had none, so every `$LINENO` after it inside that
#     body moves down a line.
#
# `sh_single_quote`'s own special case shows through: a translation that is
# nothing but a quote is written `\'`, not `''\'''`.
#
# Not covered here: a `$'…'` inside a `${ … }` body, where bash 5.2 has a defect
# it has already fixed for 5.3. See `known-issues.md`,
# `TD-OILS-AN-ANSI-C-STRING-IS-NOT-REQUOTED-AFTER-A-BRACE-BODY-TRANSLATES-IT`.
#
# Verified against bash 5.2.37.

p() { printf '%-46s -> [%s]\n' "$1" "$2"; }
e() { ( eval "$1" ) 2>&1; echo "  rc=$?"; }

echo "=== an ANSI-C string in arithmetic is an error naming the translation ==="
e 'echo $(( $'\''a'\'' ))'
e 'echo $(( $'\''5'\'' ))'
e 'echo $(( 1 + $'\''2'\'' ))'
# The escapes are resolved first, so the error quotes the resolved text.
e 'echo $(( $'\''a\tb'\'' ))'
e 'echo $(( $'\''a\x27b'\'' ))'
# A NUL ends the string.
e 'echo $(( $'\''a\0b'\'' ))'
# And the whole-value special case of the re-quoting.
e 'echo $(( $'\''\x27'\'' ))'
# `$[ … ]` is a grouping construct too.
e 'echo $[ $'\''a'\'' ]'

echo "=== a locale string in arithmetic is rescanned as arithmetic ==="
a=5
p 'a name'            "$(( $"a" ))"
p 'an expression'     "$(( $"1+2" ))"
p 'beside an operand' "$(( 1 + $"2" ))"
p 'an unset name'     "$(( $"nosuch" ))"
p 'in $[ … ]'         $[ $"a" ]

echo "=== the translation is what the printback shows ==="
f() {
  : $(( $'\x27' ))
  : $(( $"1+2" ))
  : $(echo $'a\047b')
  : $(echo $"x")
  : <(echo $'a\tb')
}
declare -f f

echo "=== and it changes the body's shape, so \$LINENO moves ==="
# The translated run carries a real newline, so the command after it inside the
# body sits one line lower than the line the whole thing was written on. The
# base is printed beside it because the interesting number is the difference.
echo "A $(echo $'x\ny'; echo L=$LINENO) base=$LINENO"
echo "B $(echo $'xy'; echo L=$LINENO) base=$LINENO"

echo "=== a matched delimiter is not a grouping construct, so nothing happens ==="
# Inside `" … "` the `$` is a literal and the quote closes the run.
p 'in double quotes'  "a$'b'"
# A backtick body is `open == close`, so its text travels untranslated — and is
# re-lexed, which translates it there instead. Same answer, different road.
p 'in a backtick'     "`echo $'a\tb'`"
p 'a backtick body keeps its lines' "`echo $'x\ny'; echo L=$LINENO`/$LINENO"

echo done

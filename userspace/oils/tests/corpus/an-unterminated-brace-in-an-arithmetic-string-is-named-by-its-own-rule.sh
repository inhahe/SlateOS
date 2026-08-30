# An arithmetic string is a word expansion of its own — bash's
# `expand_arith_string` hands the text to `expand_word_internal` before
# `evalexp` sees it — and an arithmetic scan, unlike a word scan, does not step
# over a `${ … }` (that wants `P_ARRAYSUB|P_DOLBRACE`, parse.y:3929). So a brace
# that never closes survives into the expansion, and is complained about there.
#
# Which of bash's two complaints it is has nothing to do with the brace. Both
# come out of `parameter_brace_expand` (subst.c:9539), which reads the *name*
# before it ever goes looking for the `}`, and the name alone settles it:
#
#   * `valid_brace_expansion_word` (subst.c:9814) accepts an identifier, an
#     all-digit positional, a one-character special parameter and a well-formed
#     `a[…]` reference — nothing else. A name it refuses is a plain "bad
#     substitution", raised before the missing brace is noticed at all.
#   * `string_extract` (subst.c:795) ends the name only at one of
#     `#%^,~:-=?+/@}` — not at a space, and not at any other character a name may
#     not hold. A name that runs to the end of the text therefore finishes with
#     bash's `c == 0`, which the `switch` at subst.c:10034 also sends to "bad
#     substitution".
#
# Only a valid name stopped by an operator reaches `extract_dollar_brace_string`
# (subst.c:9910), and only that call says ``no closing `}'`` — naming the whole
# arithmetic string, because that is the string it was handed.
#
# Verified against bash 5.2.37.

e() { ( eval "$1" ) 2>&1; echo "  rc=$?"; }

echo "=== the name decides, not the brace ==="
# A usable name with an operator after it: the missing brace is what is reported.
e 'echo $(( ${x:- ))'
e 'echo $(( ${x- ))'
e 'echo $(( ${x+ ))'
e 'echo $(( ${x= ))'
e 'echo $(( ${x? ))'
e 'echo $(( ${x# ))'
e 'echo $(( ${x% ))'
e 'echo $(( ${x^ ))'
e 'echo $(( ${x/ ))'
e 'echo $(( ${x@ ))'
e 'echo $(( ${xy- ))'
e 'echo $(( ${_- ))'
e 'echo $(( ${9- ))'
e 'echo $(( ${a[0]:- ))'
# Nothing after the name at all — the name ran to the end of the text — so it is
# a plain bad substitution even though the name itself is fine.
e 'echo $(( ${x ))'
e 'echo $(( ${ ))'
e 'echo $(( ${x! ))'
e 'echo $(( ${x. ))'
e 'echo $(( ${x y ))'
# A name the brace rules refuse, complained about before the brace is missed.
e 'echo $(( ${ x- ))'
e 'echo $(( ${x[:- ))'
# `string_extract` steps over a balanced subscript and a `\c` pair, so neither
# `:` below ends a name — the whole remainder is the name, and it is not valid.
e 'echo $(( ${a[x:- ))'
e 'echo $(( ${x\:- ))'

echo "=== the one-character special parameters are re-scanned ==="
# `-`, `?` and `#` are names *and* operator characters, so bash steps over the
# first one and looks again (VALID_SPECIAL_LENGTH_PARAM, subst.c:9605).
e 'echo $(( ${-:- ))'
e 'echo $(( ${?:- ))'
e 'echo $(( ${#:- ))'
e 'echo $(( ${- ))'
e 'echo $(( ${? ))'
e 'echo $(( ${# ))'
e 'echo $(( ${#x ))'
e 'echo $(( ${#x- ))'
# `@` is a name of its own (subst.c:9577), so what follows it is the operator —
# the one form where a trailing space is the whole difference.
e 'echo $(( ${@ ))'
e 'echo $(( ${@))'
e 'echo $(( ${* ))'
# An indirection may be through `# ? @ *` (VALID_INDIR_PARAM, subst.c:117) — and
# deliberately not through `-`.
e 'echo $(( ${!@ ))'
e 'echo $(( ${!# ))'
e 'echo $(( ${!- ))'
e 'echo $(( ${#@ ))'
e 'echo $(( ${#* ))'

echo "=== an indirection is resolved before the brace is missed ==="
# `parameter_brace_expand_indir` (subst.c:7621) runs first, so its own failures
# come out in the missing brace's place — but only its own: the *value* behind
# the pointer is never read, which is why `set -u` says nothing here.
e 'y=z; echo $(( ${!y- ))'
e 'set -u; y=z; echo $(( ${!y- ))'
e 'echo $(( ${!x- ))'
e 'y=; echo $(( ${!y- ))'
e 'y="a b"; echo $(( ${!y- ))'
e 'y=1abc; echo $(( ${!y- ))'
e 'a=(x); x=5; echo $(( ${!a[0]- ))'
e 'echo $(( ${!nope[0]- ))'
# A nameref answers with its target's *name*, which it has even when unset.
e 'declare -n r=q; echo $(( ${!r- ))'
# With nothing after the name there is no indirection to try — the switch on
# `c == 0` refuses it first.
e 'y=z; echo $(( ${!y ))'

echo "=== what gets named is the arithmetic string ==="
# Not the word around it, and not the `${ … }` inside it.
e 'echo $[ ${x:- ]'
e 'echo p$[ ${x:- ]q'
e 'echo "a$(( 1 + ${x:- ))b"'
e 'echo $(( 1 + ${x:- + 2 ))'
e '(( ${x:- )); echo "after=$?"'
e 'let " ${x:- "'
e 'let " ${ "'
# A nested arithmetic string is one of its own and names itself.
e 'echo $(( 1 + $(( ${x:- )) ))'

echo "=== the expression is not evaluated afterwards ==="
# One diagnostic, not two: the hole the failure left never reaches the evaluator.
e 'echo $(( ${x:- + ${y:- ))'
e 'echo $(( ${ + ${ ))'

echo "=== the two are not the same class of failure ==="
# A plain bad substitution is an expansion error, which posix mode makes fatal;
# the missing-brace one is raised with a bare `exp_jump_to_top_level (DISCARD)`
# that the posix hook never sees, so it discards the command and no more.
( set -o posix; echo $(( ${x:- )); echo "posix-no-closing-after" ) 2>&1
( set -o posix; echo $(( ${ )); echo "posix-bad-sub-after" ) 2>&1
( echo $(( ${x:- )); echo "plain-no-closing-after" ) 2>&1
( echo $(( ${ )); echo "plain-bad-sub-after" ) 2>&1

echo done

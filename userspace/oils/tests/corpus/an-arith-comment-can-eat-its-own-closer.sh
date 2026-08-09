# A `#` comment inside a one-line `$(( … ))` swallows the closing `))`.
#
# bash reads the construct twice. The parser matches the parentheses knowing
# nothing about comments and hands the expander a body; the expander then
# re-scans the *word source* with a scanner that does honour `#`. When the
# comment runs to the end of the line the `))` is inside it and never found, so
# what is reported is a "no closing `)'" complaint naming the whole word rather
# than an arithmetic error about the body.

echo '--- the plain form'
echo $(( #5 ))
echo "rc=$?"

echo '--- the word named is the whole word, quoting and neighbours included'
echo "A$(( #5 ))B"
echo "rc=$?"
p"A$(( #5 ))B"q
echo "rc=$?"

x=5

echo '--- nesting reports the outermost word, once'
echo $(( 1 + $(( 2 # x )) ))
echo "rc=$?"

echo '--- the verdict is reached while the word is *scanned*, before expansion'
echo "${x:-$(( #5 ))}"
echo "rc=$?"
echo "${x/a/$(( #5 ))}"
echo "rc=$?"
a=(1 2)
echo "${a[$(( #5 ))]}"
echo "rc=$?"
echo "${x:1:$(( #5 ))}"
echo "rc=$?"

echo '--- so nothing in the word gets to run first'
echo "$(echo ran)$(( #5 ))"
echo "rc=$?"

echo '--- and a word never reached is never scanned, so this reports nowhere'
false && echo $(( #5 ))
echo "rc=$?"

echo '--- a $( ) body is its own business: it reports, the outer word does not'
echo "$( echo $(( #5 )) )"
echo "rc=$?"

echo '--- a subscript is read out of the assignment, so the assignment is named'
a[$(( #5 ))]=1
echo "rc=$?"
a[$(( #5 ))]=$(( 1 ))
echo "rc=$?"
declare a[$(( #5 ))]=1
echo "rc=$?"
a=([$(( #5 ))]=1)
echo "rc=$?"
x=1 a[$(( #5 ))]=1 true
echo "rc=$?"

echo '--- the value, though, is a word and names itself'
a[1]=$(( #5 ))
echo "rc=$?"

echo '--- and the complaint comes once, not once per consequence'
declare -A m
m[$(( #5 ))]=1
echo "rc=$?"

echo '--- body index 0 is preceded by the `(`, so this is arithmetic'
echo $((#5))
echo "rc=$?"

echo '--- a `#` after an operator, an identifier or a backslash is not a comment'
echo $(( 1 +#2 ))
echo "rc=$?"
echo $(( x#2 ))
echo "rc=$?"
echo $(( 1 \# 2 ))
echo "rc=$?"

echo '--- quoting protects it'
echo $(( "a # b" ))
echo "rc=$?"
echo $(( 'a # b' ))
echo "rc=$?"

echo '--- but a ${ } does not'
echo $(( ${x # b} ))
echo "rc=$?"

echo '--- $[ ] is read by an extractor with no comment rule at all'
echo $[ #5 ]
echo "rc=$?"

echo '--- a newline closes the comment, so the closer is found after all'
echo $(( 1 #x
 + 2 ))
echo "rc=$?"

# This one is DISCARD-class, and unusually it is errexit-only: bash raises it
# from the word scanner rather than from the parameter expander, so posix
# mode's "an expansion error ends the shell" hook never sees it. That is the
# mirror image of an ordinary arithmetic error, which posix mode makes fatal
# and errexit lets through.
echo '--- posix mode complains and carries on'
set -o posix
echo $(( #5 ))
echo "posix carries on"
set +o posix

echo done

# `extract_delimited_string`'s "ran out of string" ending is a `report_error` and
# a longjmp *only* when `no_longjmp_on_fatal_error` is clear; when it is set the
# scan just hands back a NULL with `*sindex` at the end (subst.c:1493-1506). The
# one caller that sets it is `expand_prompt_string` (parse.y:6094), so under
# `PS1`/`PS4`/`${x@P}` the same comment is silent and the expansion simply keeps
# what it had built before the `$((`, dropping the rest of the string with it.
echo '--- under a prompt the very same comment is silent'
pa='A$(( #5 ))B';   printf 'p1 [%s] rc=%s\n' "${pa@P}" "$?"
pb='A$(( 1 # ))B';  printf 'p2 [%s] rc=%s\n' "${pb@P}" "$?"
pc='$(( #5 ))';     printf 'p3 [%s] rc=%s\n' "${pc@P}" "$?"

echo '--- a newline further down the string closes it after all, so it is arithmetic again'
pd='A$(( 1 #
+ 2 ))B';          printf 'p4 [%s] rc=%s\n' "${pd@P}" "$?"

# Here the count closes on a `)` that is not the arithmetic's own, so the extent
# does not end `)`: `param_expand` takes `goto comsub` and runs the text as a
# command substitution, leaving what the count never reached in the word.
echo '--- and a closer found late makes it a command substitution instead'
pe='A$(( 1 # ))
next )) B';        printf 'p5 [%s] rc=%s\n' "${pe@P}" "$?"
pf='A$(( 1 # ))
echo Z )) B';      printf 'p6 [%s] rc=%s\n' "${pf@P}" "$?"

# The `${ … }` scan reads a `$((` through the very same count — its `$(` row is
# `string[i] == '$' && string[i+1] == LPAREN` calling `extract_command_subst`
# (subst.c:1894-1903). Under a prompt the count is silent and merely overruns the
# scan's index, so `CHECK_STRING_OVERRUN` breaks the loop with `c = 0` and the
# brace is the thing left with nothing to close: the complaint that comes out is
# the *brace's*, naming the whole word. Outside a prompt the count reports and
# longjmps first, so the brace never gets an ending of its own — one complaint
# either way, but not the same one.
echo '--- inside a brace it is the brace that cannot close'
pg='A${x:-$(( #5 ))}B';   printf 'p7 [%s] rc=%s\n' "${pg@P}" "$?"
ph='A${x:-$(( 1 # ))}B';  printf 'p8 [%s] rc=%s\n' "${ph@P}" "$?"
pi='A${x:-${x:-$(( #5 ))}}B'; printf 'p9 [%s] rc=%s\n' "${pi@P}" "$?"
# A `$[ … ]` is not that row — `string[i+1]` is `[` — so the scan never reads it
# and walks its bytes plainly.
pj='A${x:-$[ #5 ]}B';     printf 'p10 [%s] rc=%s\n' "${pj@P}" "$?"
# A comment the scan steps over inside a `' … '` run is not read at all.
pk="A\${x:-'p\$(( #5 ))q'}B"; printf 'p11 [%s] rc=%s\n' "${pk@P}" "$?"

echo '--- PS4 is the same door'
PS4='+A$(( #5 ))B '
set -x
: marker
set +x

echo '--- errexit ends the shell, so nothing below runs'
set -e
echo $(( #5 ))
echo "errexit never reaches here"

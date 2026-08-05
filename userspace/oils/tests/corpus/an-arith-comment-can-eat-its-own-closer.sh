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

echo '--- errexit ends the shell, so nothing below runs'
set -e
echo $(( #5 ))
echo "errexit never reaches here"

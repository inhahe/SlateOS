# A tilde expansion's *result* is quoted; the rest of the word is not.
#
# bash marks what `~` expanded to the same way it marks the value of a parameter
# expansion inside double quotes, so the home directory is never re-read as a
# pattern — while the tail of the word after the tilde-prefix stays ordinary
# unquoted text and still globs. `HOME` is set to a glob here so the two halves
# can be told apart, which is the only way to see the distinction at all.
#
# The prefix runs from just after `~` to the first `/`; an unresolvable prefix
# (`~nosuchuser`) is no expansion at all and the whole word stays as written.

mkdir -p home && : > home/one && : > home/two
: > homeX

echo "=== the result is not globbed"
HOME='home*'
echo ~
echo ~ ~

echo "=== but the tail of the word is"
HOME=home
echo ~/*
echo ~/o*

echo "=== and a metacharacter in the result stays literal in a pattern"
HOME='home*'
[[ 'home*' == ~ ]]; echo "literal=$?"
[[ homeX == ~ ]]; echo "asglob=$?"
[[ 'home*/x' == ~/x ]]; echo "literal_sub=$?"

echo "=== the tail of a pattern is still live"
HOME=home
[[ home/one == ~/* ]]; echo "tail_glob=$?"
[[ 'home/*' == ~/* ]]; echo "tail_lit=$?"

echo "=== other operand positions expand too"
HOME=/xyz
[[ ~ == /xyz ]]; echo "lhs=$?"
[[ /xyz == ~ ]]; echo "rhs=$?"
[[ -n ~ ]]; echo "unary=$?"
[[ ~ =~ ^/xyz$ ]]; echo "regex=$?"
case ~ in /xyz) echo "case=yes";; *) echo "case=no";; esac
echo ~ ~/a "~" '~'

echo "=== a tilde that is not a prefix, or does not resolve"
echo a~b a=~b
echo ~nosuchuser ~nosuchuser/x
[[ x == a~b ]]; echo "mid=$?"
[[ '~' == '~' ]]; echo "quoted=$?"

echo "=== assignment context: after the value's start and after each colon"
HOME=/h
v=~; echo "$v"
v=~/a:~/b; echo "$v"
v=x:~; echo "$v"
v="~"; echo "$v"

rm -f homeX home/one home/two
rmdir home

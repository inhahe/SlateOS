# `<&WORD` and `>&WORD` decide what the word is by asking whether every byte of
# it is a *digit* — not whether it parses as a number. The two answers differ in
# three places, and each one picks a different error out of the hat:
#
#   * the empty word is vacuously all-digits, so it is a *bad descriptor*, not a
#     nonsensical one — and on the output side it is not a redirect to a file
#     with an empty name either;
#   * a sign makes a word non-numeric, so `+3` and `-1` are ambiguous however
#     well they parse;
#   * a run of digits too long for a descriptor is bad, not ambiguous.
#
# What the message then *names* is a second question, with three answers of its
# own:
#
#   * the ambiguous one names the word's expansion;
#   * the bad-descriptor one names a *number*, when the word was a bare run of
#     digits short enough to be one — `<&007` says `7`, not `007`, and says it
#     whatever the redirector is;
#   * and otherwise it names the word exactly as written, quotes and all — but
#     only when the redirector is the one the operator supplies by itself, 0 for
#     `<&` and 1 for `>&`. Write any other redirector and that number is named
#     instead of the word.
#
# So a single word can be reported three different ways depending only on how it
# was spelled and where it was put, which is what the last two sections are for.
#
# The fourth answer to the first question, `-`, is missing on purpose: closing
# fd 0/1/2 for the duration of one command is not modelled yet, so `echo hi >&-`
# still prints (see known-issues TD-OILS-TRANSIENT-CLOSE-OF-A-STD-FD-IS-A-NO-OP).
# For a related reason no case below writes `N<&M` for a valid-looking M with N
# other than 0, nor `N>&N` — an input dup onto a non-zero descriptor does not
# check its source yet, and a dup of a descriptor onto *itself* needs no source
# to check but is checked anyway. See TD-OILS-DUP-ONTO-A-NON-STD-FD-IS-WRONG.
#
# Stderr is collected and replayed at the end so it can be compared in a fixed
# place; nothing here prints a pid, so it is replayed unfiltered.
exec 3>&2 2>err
e=""
exec 5>/dev/null

echo "=== the empty word is a descriptor that is not there"
read -r l <&"$e"; echo "  in  rc=$?"
echo hi >&"$e";   echo "  out rc=$?"

echo "=== so is one that is all digits and much too long"
read -r l <&"99999999999999999999"; echo "  in  rc=$?"
echo hi   >&"99999999999999999999"; echo "  out rc=$?"

echo "=== but an open one is fine"
echo ok >&"5"; echo "  out rc=$?"

echo "=== a signed word is not a descriptor at all"
read -r l <&"+3"; echo "  +3 rc=$?"
read -r l <&"-1"; echo "  -1 rc=$?"

echo "=== nor is anything else with a non-digit in it"
read -r l <&"abc"; echo "  abc rc=$?"
read -r l <&"1x";  echo "  1x  rc=$?"
read -r l <&"0x3"; echo "  0x3 rc=$?"
read -r l <&" 1 "; echo "  ' 1 ' rc=$?"

echo "=== on fd 1 a non-descriptor is a >&file redirect instead"
echo one >&"fileA";  echo "  fd1 rc=$?"
echo two >&"6-";     echo "  6-  rc=$?"
echo "  fileA=[$(cat fileA)] 6-=[$(cat ./6-)]"

echo "=== but only on fd 1"
echo three 2>&"fileB"; echo "  fd2 rc=$?"
echo four 3>&"fileC";  echo "  fd3 rc=$?"
echo "  fileB exists: $([ -e fileB ] && echo yes || echo no)"

echo "=== an ambiguous word is named the same wherever it is put"
read -r l 7<&"abc";  echo "  7<&  rc=$?"
echo hi   2>&"abc";  echo "  2>&  rc=$?"
echo hi   7>&"abc";  echo "  7>&  rc=$?"

echo "=== a bad one is named only where the operator's own descriptor is"
read -r l 0<&"$e"; echo "  0<& rc=$?"
read -r l 1<&"$e"; echo "  1<& rc=$?"
read -r l 2<&"$e"; echo "  2<& rc=$?"
read -r l 7<&"$e"; echo "  7<& rc=$?"
read -r l 9<&"$e"; echo "  9<& rc=$?"
echo hi   1>&"$e"; echo "  1>& rc=$?"
echo hi   0>&"$e"; echo "  0>& rc=$?"
echo hi   2>&"$e"; echo "  2>& rc=$?"
echo hi   7>&"$e"; echo "  7>& rc=$?"

echo "=== which holds for every way of being a bad one"
read -r l 7<&"99999999999999999999"; echo "  7<& too long rc=$?"
echo hi   2>&"99999999999999999999"; echo "  2>& too long rc=$?"
read -r l  <&"9"; echo "  0<& merely closed rc=$?"
echo hi   1>&"9"; echo "  1>& merely closed rc=$?"
echo hi   2>&"9"; echo "  2>& merely closed rc=$?"
echo hi   7>&"9"; echo "  7>& merely closed rc=$?"

echo "=== bare digits are a number, and a number is named as one"
read -r l <&9;   echo "  0<&9   rc=$?"
read -r l <&007; echo "  0<&007 rc=$?"
echo hi   >&007; echo "  1>&007 rc=$?"
echo hi  2>&007; echo "  2>&007 rc=$?"

echo "=== one quote or backslash is enough to make them a word again"
read -r l <&9"";  echo "  0<& quoted rc=$?"
echo hi  2>&9"";  echo "  2>& quoted rc=$?"
read -r l <&\9;   echo "  0<& escaped rc=$?"
echo hi  2>&\9;   echo "  2>& escaped rc=$?"

exec 2>&3 3>&-
echo "=== what went to stderr"
cat err

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
# The two messages even quote differently: the ambiguous one names the word's
# *expansion*, the bad-descriptor one the word as it was written.
#
# The fourth answer, `-`, is missing on purpose: closing fd 0/1/2 for the
# duration of one command is not modelled yet, so `echo hi >&-` still prints
# (see known-issues TD-OILS-TRANSIENT-CLOSE-OF-A-STD-FD-IS-A-NO-OP). Every
# redirector below is the operator's default, for a second reason of the same
# kind — see TD-OILS-DUP-BADFD-NAMES-THE-WRONG-THING.
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

exec 2>&3 3>&-
echo "=== what went to stderr"
cat err

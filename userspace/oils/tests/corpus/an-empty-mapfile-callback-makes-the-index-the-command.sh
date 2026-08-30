# `mapfile -C ""` is still a callback. bash never asks whether the callback text
# is *empty* — only whether `-C` was given at all, which is a null-pointer test
# on a `char *`:
#
#   if (callback && line_count && (line_count % callback_quantum) == 0)
#                                             (builtins/mapfile.def:206)
#
# and the text is pasted into a command line unconditionally:
#
#   snprintf (execstr, execlen, "%s %d %s", callback, curindex, qline);
#                                             (builtins/mapfile.def:131)
#
# So an empty callback yields the command line ` INDEX LINE`, whose command
# *word* is the index — which is normally not a command, so each fire is a
# "command not found". The array is filled all the same.

echo "=== an empty callback fires once per line, with the index as the command"
mapfile -t -C '' -c 1 a <<< $'x\ny'
echo "rc=$?"
declare -p a

echo "=== the index is the one about to be stored, so -O moves it"
mapfile -t -C '' -O 5 -c 1 b <<< $'x\ny'
echo "rc=$?"
declare -p b

echo "=== the quantum still says which lines fire"
mapfile -t -C '' -c 2 c <<< $'1\n2\n3\n4\n5'
echo "rc=$?"

echo "=== a callback of one space is no different"
mapfile -t -C ' ' -c 1 d <<< 'x'
echo "rc=$?"

echo "=== the line is passed as one argument, quoted, and -t still strips"
mapfile -C '' -c 1 e <<< $'p q'
echo "rc=$?"
declare -p e

echo "=== …and with no lines there is nothing to fire for"
mapfile -t -C '' -c 1 f < /dev/null
echo "rc=$? n=${#f[@]}"

echo "=== a non-empty callback is unchanged: index and line are appended to it"
mapfile -t -C 'echo cb' -c 1 g <<< $'x\ny'
echo "rc=$?"

echo "still here"

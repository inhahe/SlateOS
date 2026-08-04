# `/dev/null` is one thing to the shell, and the file tests have to agree with
# the redirections about what it is.
#
# `> /dev/null` discards and `< /dev/null` is EOF — on a host without the node,
# because the shell routes the open elsewhere. The file primaries stat the
# literal path instead, so without the same routing they answer about whatever
# that path resolves to: nothing, or a stray regular file left there by a
# redirect from before the routing existed, whose size then makes `-s` true.
#
# The device answers, measured: `-e -c -r -w -O -G` hold and nothing else does.
# `-c` is the point of it — the null device is a character device, never a
# regular file, so `-f` and `-s` are false however the host stores it.

p() { printf '  [%s]' "$@"; echo; }

echo "=== every file primary, against the device"
for op in -e -a -c -r -w -f -d -b -p -S -s -u -g -k -L -h -x -O -G; do
  if [ $op /dev/null ]; then printf '  %s yes\n' "$op"; else printf '  %s no\n' "$op"; fi
done

# The operator of a `[[ ]]` primary is syntax, not a word, so these cannot be
# written as a loop the way the `[` ones above can.
echo "=== [[ ]] draws on the same evaluator, so it gives the same answers"
[[ -e /dev/null ]]; echo "  -e st=$?"
[[ -c /dev/null ]]; echo "  -c st=$?"
[[ -r /dev/null ]]; echo "  -r st=$?"
[[ -w /dev/null ]]; echo "  -w st=$?"
[[ -f /dev/null ]]; echo "  -f st=$?"
[[ -s /dev/null ]]; echo "  -s st=$?"
[[ -d /dev/null ]]; echo "  -d st=$?"

echo "=== the string primaries ask about the operand, not about a file"
[ -n /dev/null ]; echo "  -n st=$?"
[ -z /dev/null ]; echo "  -z st=$?"
[[ -n /dev/null ]]; echo "  [[ -n ]] st=$?"

# A real file of the same name, made here, proves the device is keyed on the
# path and not on the name: `dev/null` is a file that exists and has a size,
# while `/dev/null` right beside it stays the device.
echo "=== a path that merely resembles it is an ordinary path"
mkdir -p dev && printf 'contents\n' > dev/null
for f in dev/null ./dev/null; do
  if [ -f "$f" ] && [ -s "$f" ]; then printf '  %s is a file\n' "$f"; else printf '  %s is not\n' "$f"; fi
done
[ -c /dev/null ]; echo "  /dev/null still -c st=$?"
[ -f /dev/null ]; echo "  /dev/null still not -f st=$?"
echo "  contents=$(cat dev/null)"

echo "=== and the redirections still behave like the device"
echo discarded > /dev/null; echo "  write st=$?"
read x < /dev/null; echo "  read st=$? [$x]"
cat < /dev/null; echo "  cat st=$?"
echo "  wc=$(wc -c < /dev/null)"

echo "=== writing does not make it into a file"
echo junk > /dev/null
[ -s /dev/null ]; echo "  -s after write st=$?"
[ -f /dev/null ]; echo "  -f after write st=$?"
read y < /dev/null; echo "  reread st=$? [$y]"

echo "=== appending, truncating and both at once"
echo more >> /dev/null; echo "  append st=$?"
: > /dev/null; echo "  truncate st=$?"
exec 7<> /dev/null; echo "  rw st=$?"
echo two >&7; echo "  write7 st=$?"
exec 7>&-

# `!`-style history expansion, the thing `set -H` switches on. The switch's own
# surface (`$-`, SHELLOPTS, `set -o`) is pinned separately by
# histexpand-option.sh; this case is about the rewriting.
#
# Two properties make this worth pinning tightly rather than sampling:
#
#   * the *echo*. bash prints the rewritten line to stderr before running it, so
#     every expansion here is observable twice — once as the echo and once as the
#     command's own output — and a wrong expansion cannot hide behind a command
#     that happens to print the same thing.
#   * the *history* the expansion sees. Entries are recorded per parse unit, so
#     inside a multi-line compound command `!!` still names the command *before*
#     it, not the line above. That is checked explicitly below.
#
# Both switches are needed: `set -H` alone does nothing while `history` is off.

set -o history
set -H

echo "=== !! recalls the previous command, echoing what it became"
echo alpha
!!

echo "=== the recalled line is re-parsed, so redirections in it apply"
echo beta
!! > /dev/null

echo "=== word designators slice the recalled command"
echo one two three
echo !!:2
echo !^ and !$
echo !!:1-2
echo !!:*

echo "=== absolute and relative event numbers"
echo gamma
!-1
!5

echo "=== prefix and substring search"
echo delta
echo epsilon
!?del?
!ec

echo "=== double quotes do NOT suppress expansion, single quotes do"
echo zeta
echo "in dquotes: !!"
echo 'in squotes: !! stays'

echo "=== a backslash suppresses it, and is eaten"
echo \!! literal

echo "=== a bare ! before space, =, ( or end of line is literal"
echo bang ! here and x!=y and trailing!

echo "=== :h :t :r :e slice a pathname out of the recalled word"
echo /usr/local/lib/foo.tar.gz
echo !$:h
echo !-2:$:t
echo dir/base.ext
echo !$:r and !$:e

echo "=== :s substitutes, :gs substitutes everywhere, :& repeats the last one"
echo aa bb aa
!!:s/aa/cc/
echo aa bb aa
!!:gs/aa/dd/
echo aa bb aa
!!:s/aa/ee/
echo aa xx aa
!!:&

echo "=== ^old^new^ is the same substitution applied to the previous line"
echo needle in haystack
^needle^pin^

echo "=== :q quotes the result as a single word"
echo eta
echo !!:q

echo "=== :p previews: the line is echoed and recorded, but not run"
echo iota
!!:p
echo "still here"

echo "=== history records the expanded form, not what was typed"
history 5

echo "=== inside a compound command, !! is still the command before the if"
echo theta
if true; then
  echo !!
fi

echo "=== a here-document body is not expanded"
cat <<XEOF
body !! stays literal
XEOF

echo "=== an unknown event is reported and the line is dropped, leaving \$? alone"
true
!nosuchcommand
echo "status after the miss: $?"

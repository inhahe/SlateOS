# bash's reader hands the scanner one physical line at a time, and writes back
# the newline a last line came without. A file that stops mid-line is therefore
# read as though it ended in a newline — so a final `\` in it is a continuation
# joining it to an input that is not there, and vanishes.
#
# A *string* is the exception. Closing it with a newline would eat its final
# backslash the same way, so bash closes it with a second backslash instead and
# the pair is one quoted, literal backslash. Everything `parse_and_execute`
# runs is a string: `eval`, a `.`/`source`d file (read whole, then parsed), a
# trap action, the `-c` command string.
#
# This file is itself read by the *stream* reader, so the string reader is
# reached here through `eval` and `.`. The stream side has a case of its own:
# a-script-file-that-stops-mid-line-is-read-as-if-it-ended-in-a-newline.sh.

echo "=== eval keeps the backslash"
eval 'echo one\'
eval 'echo two\\'
eval 'echo three\\\'
eval 'echo four\\\\'

echo "=== and it is quoted, so expansion leaves exactly one"
eval 'printf "[%s]\n" five\'
eval 'v=six\'; echo "[$v]"
eval 'echo "seven"\'
eval "echo 'eight'\\"
eval 'echo nine\' | cat

echo "=== a sourced file is a string too"
printf 'echo ten\\' > f1; . ./f1
printf 'echo eleven\\\\' > f2; . ./f2
printf 'v=twelve\\' > f3; . ./f3; echo "[$v]"
# The same file with its newline written stops being a candidate at all.
printf 'echo thirteen\\\n' > f4; . ./f4

echo "=== it is the word that changes, not just its text"
# `esac\` is not the reserved word, so the `case` is never finished.
eval 'case a in a) echo hit;; esac' ; echo "rc=$?"
eval 'case a in a) echo hit;; esac\' 2>/dev/null ; echo "rc=$?"
# Nor is `then\`, and nor is `fi\`.
eval 'if true; then echo yes; fi' ; echo "rc=$?"
eval 'if true; then echo yes; fi\' 2>/dev/null ; echo "rc=$?"
# An assignment stops being one, since its word no longer ends at the value.
eval 'w=set; echo "[$w]"' ; echo "rc=$?"

echo "=== an even run of backslashes has no live continuation to lose"
# So both readers agree, whatever the input ends in.
eval 'echo a\\'
printf 'echo a\\\\' > f5; . ./f5
eval 'echo b'
printf 'echo b' > f6; . ./f6

echo "=== a trap action is parse_and_execute as well"
trap 'echo bye\' EXIT

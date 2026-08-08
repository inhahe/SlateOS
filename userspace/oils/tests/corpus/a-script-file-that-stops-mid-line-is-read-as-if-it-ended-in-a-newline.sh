# !! THIS FILE DELIBERATELY HAS NO FINAL NEWLINE, and its last line ends on a
# !! lone backslash. Do not add one: the missing newline is the whole case.
#
# bash's `shell_getc` (parse.y) reads one physical line at a time and writes
# back the newline a line came without, so a script that stops mid-line is read
# as though it ended in one. The final `\` is therefore an ordinary line
# continuation — onto an input that is not there — and is deleted with the
# newline it quoted, leaving the word, the command and the file all complete.
#
# A *string* is closed the other way (with a second backslash, so the pair is a
# literal one); that half is
# the-two-readers-close-an-unterminated-last-line-differently.sh.
echo "=== a file that stops mid-line still runs its last command"
printf '[%s]\n' one
echo "=== and the trailing backslash goes with the newline the reader adds"
echo two\
# A NUL byte in shell source is not an error and is not data: the reader throws
# it away before anything can see it, and a *script file* whose first line has
# one is refused outright. Measured against bash 5.2.
#
#   * `shell_getc` drops a NUL as it reads it, so one never reaches a token —
#     `echo a<NUL>b` prints `ab`, and a line that is nothing but a NUL is a blank
#     line. It is the reader that does this, so it holds for every way source
#     arrives: a script file, `-c`, `eval`, `.`/`source`, and a piped REPL;
#   * `open_shell_script` refuses a *binary* script before parsing a byte of it —
#     status 126, and a line naming the script twice, because `$0` is by then the
#     script itself;
#   * binary is bash's `check_binary_file`: an ELF image, or a NUL before the
#     first newline. The window is exactly 80 bytes, so a NUL at byte 79 of the
#     first line is refused and one at byte 80 is not — and one *past* the first
#     newline never counts however early it is;
#   * a `#!` line does not exempt a file from that check;
#   * and only that reader refuses. `source` reads the same file happily, NULs
#     and all, and so does a piped REPL: the gate is on the script the shell was
#     *invoked* on and nowhere else.
#
# Nothing here needs folding: `$0` in every diagnostic is the operand as written,
# which is the same word for both shells.
#
# `awk` builds the sized files because a NUL cannot survive a shell variable —
# `printf 'x\000y'` writes one, but only straight to a file.
n() { printf 'echo a\000b\n' > "$1"; }

echo "=== a NUL past the first newline is dropped, not carried into a word"
printf 'echo one\necho a\000b\necho two\n' > late.sh
"$BASH" late.sh; echo "rc=$?"

echo "=== the same holds for every other way source is read"
n first.sh
. ./first.sh; echo "rc=$?"
printf 'v=p\000q\necho "[$v] ${#v}"\n' > var.sh
. ./var.sh
"$BASH" < first.sh; echo "rc=$?"
eval "$(printf 'echo e\000f\n' | tr -d '\000')"

echo "=== but a script file with one in its first line is refused before parsing"
"$BASH" first.sh; echo "rc=$?"
printf '\000echo hi\n' > lead.sh
"$BASH" lead.sh; echo "rc=$?"
printf '\177ELF\002\001\001\000pad\n' > elf.bin
"$BASH" elf.bin; echo "rc=$?"

echo "=== a #! line does not excuse it"
printf '#!/bin/sh\000\necho z\n' > shb.sh
"$BASH" shb.sh; echo "rc=$?"

echo "=== the window is 80 bytes wide, exactly"
awk 'BEGIN{for(i=0;i<79;i++)printf "#"; printf "%c\necho ran-79\n", 0}' > w79.sh
awk 'BEGIN{for(i=0;i<80;i++)printf "#"; printf "%c\necho ran-80\n", 0}' > w80.sh
"$BASH" w79.sh; echo "rc=$?"
"$BASH" w80.sh; echo "rc=$?"

echo "=== an empty file and a lone NUL are opposite answers"
printf '' > empty.sh
"$BASH" empty.sh; echo "rc=$?"
printf '\000' > justnul.sh
"$BASH" justnul.sh; echo "rc=$?"

echo "=== the refused file is still perfectly good source to read"
( . ./lead.sh; . ./shb.sh ); echo "rc=$?"

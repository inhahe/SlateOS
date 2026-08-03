# A command file the OS refuses to run as a program image is not simply an
# error: the shell looks at what is in it and either runs it *itself* or says
# it cannot. Measured against bash 5.2.
#
#   * a text file with no `#!` line is fed to a fresh copy of the running
#     shell, with the script in front of the original arguments — so `$0` is
#     the script, `$#` counts only the real arguments, and it is the *same*
#     shell that reads it, not `/bin/sh`;
#   * `$0` is the file as the shell named it to itself: the word as typed when
#     the word spelled a path, the `$PATH` hit otherwise;
#   * the degenerate texts count as text — an empty file, a single line with no
#     terminator, and a file whose NULs are all past the first newline (the
#     reader drops those, so they never reach a word);
#   * a binary is refused, with a line worded against the file rather than
#     against the errno, and status 126. `exec` adds a second line of its own;
#   * this holds wherever an external command is started: on its own, as a
#     pipeline stage, and as a `&` job.
#
# `#!` files are deliberately absent: the OS honours those itself wherever it
# honours them at all, and where it does not, running the file as *shell*
# source would be wrong (TD-OILS-SHEBANG-INTERPRETER).
#
# Diagnostics name the shell — `$0`, the path it was invoked as
# (TD-OILS-DOLLAR-ZERO-ARGV0) — and the scratch directory differs per run, so
# both are folded away. The shell-name pattern must not be `[^:]*`: a Windows
# path carries a drive-letter colon of its own.
here=$PWD
sq() { sed -e 's/^.*: line [0-9]*: /SH: /' -e "s|$here|DIR|g"; }

printf 'echo "0=$0 n=$# args=$*"\n' > tell.sh
printf '' > empty.sh
printf 'echo no-trailing-newline' > nonl.sh
printf 'echo late\necho x\000y\n' > latenul.sh
printf '\177ELF\002\001\001\000padding\n' > elf.bin
printf 'abc\000def\nghi\n' > nul.bin
chmod +x ./*.sh ./*.bin

echo "=== a shebangless text file is run by the shell itself"
./tell.sh a "b c"; echo "rc=$?"
./empty.sh; echo "rc=$?"
./nonl.sh; echo "rc=$?"
./latenul.sh; echo "rc=$?"

echo "=== and it is the same shell, not /bin/sh"
export PV="$BASH_VERSION"
printf 'if [ "$BASH_VERSION" = "$PV" ]; then echo same-shell; else echo other-shell; fi\n' > ver.sh
chmod +x ver.sh
./ver.sh

echo "=== \$0 is the file as the shell named it to itself"
./tell.sh | sq
"$here/tell.sh" | sq
# One `$PATH` entry, because the *separator* is the host's and not the shell's:
# prepending with `:` would be a single unsplit entry on a Windows host. And a
# real assignment inside a subshell rather than a command prefix, because osh
# does not yet let a prefix assignment reach its own `$PATH` search
# (TD-OILS-PREFIX-PATH-LOOKUP).
( PATH="$here"; tell.sh ) | sq

echo "=== the environment reaches it like any other child"
printf 'echo "V=$V"; type -t f; f\n' > env.sh
chmod +x env.sh
f() { echo from-exported-function; }
export -f f
V=set ./env.sh

echo "=== a binary is refused, and exec says so twice"
{ ./elf.bin; echo "rc=$?"; } 2>&1 | sq
{ ./nul.bin; echo "rc=$?"; } 2>&1 | sq
( exec ./elf.bin; echo not-reached ) 2>&1 | sq
echo "=== every way of starting an external command agrees"
./tell.sh p | cat
echo x | ./tell.sh q
./tell.sh r &
wait
{ ./elf.bin | cat; echo "rc=${PIPESTATUS[0]}"; } 2>&1 | sq

echo "=== a script can start another one"
printf './tell.sh from-outer\n' > outer.sh
chmod +x outer.sh
./outer.sh

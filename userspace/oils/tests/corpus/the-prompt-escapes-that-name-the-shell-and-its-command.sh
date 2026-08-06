# `\#`, `\!` and `\s` are the three prompt escapes that describe the shell
# rather than its surroundings, and each of them names something subtler than it
# looks.
#
# `\#` is the *reader's* count: it advances once per top-level command, between
# parsing one and running it, so everything reached from inside a command shares
# that command's number — a function body, a loop body, a group, a subshell, a
# `$( … )`, an `eval` string, a sourced file, a trap action. It is not a count of
# commands executed, and not a count of lines: two commands written on one line
# share a number, and a compound command spanning ten lines has one.
#
# The count also runs one ahead of what is displayed. bash keeps it that way for
# `PS1`, which is expanded *before* the command it introduces, and subtracts one
# back off everywhere else — so `${p@P}` and `PS4` both name the command they are
# expanded for, and agree with each other.
#
# `\!` is the history number, which is `$HISTCMD` — except that a shell which has
# recorded nothing shows 1 rather than 0, which is every script.
#
# `\s` is the name the shell was *started* under, not `$0`. The two are the same
# for a `-c` name operand and for a `BASH_ARGV0=` assignment, and differ for a
# script, which is the ordinary case: opening a script moves `$0` to its path and
# leaves the shell answering to its own `argv[0]`. That last one cannot be
# compared between two shells by value — they are different programs with
# different names — so it is compared as the property it is.

p='\#'
h='\!'
s='\s'

echo "=== the number advances once per top-level command"
echo "  a=${p@P}"
echo "  b=${p@P}"
# Two commands on one line are one unit of the reader's, so one number; and a
# `&&` or a pipeline is one command however many processes it takes.
echo "  c=${p@P}"; echo "  d=${p@P}"
echo "  e=${p@P}" && echo "  f=${p@P}"
echo "  g=${p@P}" | cat

echo "=== …and a command that contains other commands is still one command"
f() { echo "  fn1=${p@P}"; echo "  fn2=${p@P}"; }
f
for i in 1 2; do echo "  loop$i=${p@P}"; done
{ echo "  grp1=${p@P}"; echo "  grp2=${p@P}"; }
( echo "  sub1=${p@P}"; echo "  sub2=${p@P}" )
echo "  cmdsub=[$(echo "${p@P}")] here=${p@P}"
eval 'echo "  ev1=${p@P}"; echo "  ev2=${p@P}"'
printf 'echo "  src1=${p@P}"\necho "  src2=${p@P}"\n' > n1.sh
. ./n1.sh
trap 'echo "  trap=${p@P}"' USR1
echo "  before=${p@P}"
kill -USR1 $$
echo "  after=${p@P}"

echo "=== a multi-line command counts once, and the count survives a subshell"
if true
then
  echo "  if=${p@P}"
fi
echo "  after-if=${p@P}"
( echo "  in-subshell=${p@P}" )
echo "  after-subshell=${p@P}"

echo "=== \`PS4\` reads the same number as \`\${p@P}\`"
# The trace line for a command and a `${p@P}` expanded by that same command name
# the same number, which is what "one ahead, and one back off" comes to.
( PS4='  +[\#] '; set -x; echo "  x=${p@P}"; set +x ) 2>&1

echo "=== the history number is 1 in a shell that records none"
echo "  h=${h@P} HISTCMD=$HISTCMD"

echo "=== the shell name is not \$0"
# A script's `$0` is the path it was opened as; `\s` stays with the shell. The
# two programs have different names, so what is compared is that they differ,
# and then each is asked for a property it has and the other does not: `$0`
# names a script file, `\s` names an interpreter. Neither is quoted literally,
# because the script's own name is whatever the runner opened it as.
[ "${s@P}" = "$0" ]; echo "  equal? rc=$?"
case ${s@P} in */*|*\\*) echo "  s has a separator";; *) echo "  s is a bare name";; esac
case ${s@P} in *.sh) echo "  s ends in .sh";; *) echo "  s does not end in .sh";; esac
case $0 in *.sh) echo "  0 names a script file";; *) echo "  0 is [$0]";; esac
# Renaming the shell moves both, and `\s` shows the base name of the new name.
( BASH_ARGV0=zzname; echo "  s=${s@P} 0=$0" )
( BASH_ARGV0=/some/where/zzpath; echo "  s=${s@P} 0=$0" )
( BASH_ARGV0=; echo "  s=[${s@P}] 0=[$0]" )

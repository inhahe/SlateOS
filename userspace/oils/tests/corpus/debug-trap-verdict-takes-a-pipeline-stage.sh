# Under `extdebug`, a non-zero DEBUG status takes away the stage it announced.
#
# Every simple-command stage of a pipeline is announced in the shell that owns
# the pipeline, left to right, before any of them starts (see
# debug-trap-announces-a-background-command.sh). Under `shopt -s extdebug` each
# of those announcements can be refused, and a refusal costs exactly the one
# stage it was announcing: that stage is never started, so the stage after it
# reads an immediate end of file and the stage before it writes into a pipe
# nobody holds. Everything else about the pipeline is unchanged — the remaining
# stages run, and `${PIPESTATUS[@]}` simply has one element fewer.
#
# Refusing the **last** stage is the exception, and it is not really a rule
# about pipelines at all: the last stage is the one the owning shell runs
# itself, so refusing it returns out of the pipeline before it has published
# anything. The pipeline answers 0 and leaves `${PIPESTATUS[@]}` exactly as the
# previous command left it — the same as refusing a lone command. The earlier
# stages have already been started by then and still run; osh waits for them
# where bash does not, so nothing here reads their output without settling
# first (see known-issues TD-OILS-DEBUG-TRAP-VERDICT-IN-A-PIPELINE).
e() { sed 's/^.*: line [0-9]*: //'; }
p() { echo "--- $1"; ( eval "$1"; echo "rc=$?" ) 2>&1 | e; }
# Refuse the Kth announcement and say which one it was.
c() { n=$((n+1)); echo "F$n:<$BASH_COMMAND>" >&2; [ "$n" != "$K" ]; }
R='n=0; shopt -s extdebug'
# `false | false` leaves a two-element [1 1] behind, so a pipeline that
# publishes nothing is plainly distinguishable from one that publishes [0]. It
# also moves the counter by two, which every K below accounts for.
D='false | false'
E='st=$? ps="${PIPESTATUS[*]}"; trap - DEBUG; echo "st=$st ps=[$ps]"'

echo "=== a refused stage drops out of the pipeline and out of PIPESTATUS"
p "$R; K=99; trap c DEBUG; $D; exit 3 | exit 4 | exit 5; $E"
p "$R; K=3;  trap c DEBUG; $D; exit 3 | exit 4 | exit 5; $E"
p "$R; K=4;  trap c DEBUG; $D; exit 3 | exit 4 | exit 5; $E"

echo "=== …and refusing the last one takes the whole pipeline's answer away"
p "$R; K=5;  trap c DEBUG; $D; exit 3 | exit 4 | exit 5; $E"
p "$R; K=4;  trap c DEBUG; $D; exit 3 | exit 5; $E"
# A lone command is refused on exactly those terms, which is the point: there
# is no separate rule for the last stage, only the one for a command the
# owning shell runs itself.
p "$R; K=3;  trap c DEBUG; $D; exit 3; $E"

echo "=== pipefail reads the stages that ran, not the one that did not"
p "$R; set -o pipefail; K=99; trap c DEBUG; $D; exit 3 | exit 4 | exit 0; $E"
p "$R; set -o pipefail; K=3;  trap c DEBUG; $D; exit 3 | exit 4 | exit 0; $E"
p "$R; set -o pipefail; K=4;  trap c DEBUG; $D; exit 3 | exit 4 | exit 0; $E"
p "$R; set -o pipefail; K=5;  trap c DEBUG; $D; exit 3 | exit 4 | exit 0; $E"

echo "=== and ! still negates whatever is left, 0 included"
p "$R; K=99; trap c DEBUG; $D; ! exit 3 | exit 5; $E"
p "$R; K=3;  trap c DEBUG; $D; ! exit 3 | exit 5; $E"
p "$R; K=4;  trap c DEBUG; $D; ! exit 3 | exit 5; $E"

echo "=== the pipe on either side of the hole"
# Refusing the middle stage does not join stage 1 to stage 3: the reader is
# gone from one pipe and the writer from the other, so `cat` sees end of file
# and `echo`'s line is discarded.
rm -f o1 o2
p "$R; K=2; trap c DEBUG; echo one | tee o1 | cat > o2; sleep 1
   for f in o1 o2; do if [ -e \$f ]; then echo \"\$f=[\$(cat \$f)]\"; else echo \"\$f=-\"; fi; done"
rm -f o1 o2
p "$R; K=1; trap c DEBUG; echo one | tee o1 | cat > o2; sleep 1
   for f in o1 o2; do if [ -e \$f ]; then echo \"\$f=[\$(cat \$f)]\"; else echo \"\$f=-\"; fi; done"

echo "=== an abandoned pipeline still ran the stages it had started"
# The settling `sleep` is the point of the shape: bash returns from the
# pipeline without waiting for the stages it forked, so the files are not there
# yet the instant it returns. Given a moment they arrive, in both shells.
rm -f o1 o2
p "$R; K=3; trap c DEBUG; echo one > o1 | tee o2 | cat; sleep 1
   for f in o1 o2; do if [ -e \$f ]; then echo \"\$f=[\$(cat \$f)]\"; else echo \"\$f=-\"; fi; done"
rm -f o1 o2

echo "=== the rest of the enclosing command carries on regardless"
p "$R; K=5; trap c DEBUG; $D; exit 3 | exit 4 | exit 5; trap - DEBUG; echo after"
p "$R; K=5; trap c DEBUG; $D; if exit 3 | exit 4 | exit 5; then echo then; else echo else; fi"
p "$R; K=5; trap c DEBUG; $D; exit 3 | exit 4 | exit 5 && echo and"
p "$R; K=5; trap c DEBUG; $D; exit 3 | exit 4 | exit 5 || echo or"

echo "=== a group stage is refused inside its own child, and only there"
# `{ … }` is not announced by the shell that owns the pipeline — the child that
# runs the stage announces the commands inside it instead. This handler says
# nothing and only counts, because the count is the one thing that can be read
# cleanly here: it is incremented in whichever shell fires, so a child's
# increments die with the child and what the owning shell has left afterwards
# is exactly the number of announcements it made itself. Printing each firing
# instead would put the child's line and the owning shell's into one stream in
# whichever order won the race — bash does not order them.
#
# One announcement per simple stage plus one for the `trap - DEBUG` that reads
# the count, so: 3 for two simple stages, 2 for one of each, 1 for two groups.
q() { n=$((n+1)); [ "$BASH_COMMAND" != "$W" ]; }
p "$R; W=; trap q DEBUG; echo one | cat; trap - DEBUG; echo \"n=\$n\""
p "$R; W=; trap q DEBUG; { echo one; } | cat; trap - DEBUG; echo \"n=\$n\""
p "$R; W=; trap q DEBUG; echo one | { cat; }; trap - DEBUG; echo \"n=\$n\""
p "$R; W=; trap q DEBUG; { echo one; } | { cat; }; trap - DEBUG; echo \"n=\$n\""

echo "=== …so refusing it costs a command there, and a whole stage here"
# Naming the command to refuse rather than counting it is what makes this
# readable, the two shells having counted separately since they parted. The
# owning shell's announcement is *for the stage*, so refusing it takes the
# stage away and `${PIPESTATUS[@]}` is one element shorter; the child's is for
# a command *inside* a stage that runs regardless, so refusing it empties the
# stage but still leaves it in the array. Neither prints `one`.
p "$R; W=;           trap q DEBUG; $D; { echo one; } | cat; $E"
p "$R; W='echo one'; trap q DEBUG; $D; { echo one; } | cat; $E"
p "$R; W=;           trap q DEBUG; $D; echo one | { cat; }; $E"
p "$R; W='echo one'; trap q DEBUG; $D; echo one | { cat; }; $E"
p "$R; W='cat';      trap q DEBUG; $D; echo one | { cat; }; $E"

echo "=== a background pipeline loses stages the same way, the last included"
# A `&` job's stages are announced by the shell that starts it, before the
# fork, so refusing one takes that stage away exactly as in the foreground —
# but the exception for the last stage is gone, because the exception was never
# about the last stage as such. It exists because a foreground pipeline's last
# stage is the one the announcing shell runs itself; here that shell forks the
# job and runs none of it, so the last stage is just another stage.
#
# Note what has to be named to refuse a stage: `$BASH_COMMAND` is the stage as
# written, redirections and all, so `echo one` would not match `echo one > o1`.
for w in '' 'echo one > o1' 'tee o2' 'cat > o3'; do
  rm -f o1 o2 o3
  p "$R; W='$w'; trap q DEBUG; echo one > o1 | tee o2 | cat > o3 & wait; echo \"job=\$?\"
     sleep 1; trap - DEBUG
     for f in o1 o2 o3; do if [ -e \$f ]; then echo \"\$f=[\$(cat \$f)]\"; else echo \"\$f=-\"; fi; done"
done
rm -f o1 o2 o3

echo "=== an exit or a return in the handler beats the verdict to it"
x() { n=$((n+1)); echo "F$n:<$BASH_COMMAND>" >&2; [ "$n" != "$K" ] || exit 9; }
p "$R; K=2; trap x DEBUG; exit 3 | exit 4 | exit 5; echo unreachable"
y() { n=$((n+1)); echo "F$n:<$BASH_COMMAND>" >&2; [ "$n" != "$K" ] || return 2; }
g() { exit 3 | exit 4 | exit 5; echo unreachable; }
p "$R; K=3; trap y DEBUG; g; echo \"st=\$?\""
p "$R; K=5; trap y DEBUG; g; echo \"st=\$?\""

# Under `extdebug`, a non-zero DEBUG status takes away the stage it announced.
#
# Every simple-command stage of a pipeline is announced in the shell that owns
# the pipeline, left to right (see debug-trap-announces-a-background-command.sh).
# Under `shopt -s extdebug` each of those announcements can be refused, and a
# refusal costs exactly the one stage it was announcing: that stage is never
# started, so the stage after it reads an immediate end of file and the stage
# before it writes into a pipe nobody holds. Everything else about the pipeline
# is unchanged — the remaining stages run and are waited for, the pipeline
# answers with the last of them, and `${PIPESTATUS[@]}` has one element fewer.
#
# The last stage is no exception. Every stage of a multi-command pipeline is run
# in a child of the shell that owns it, so there is no stage that shell is
# already inside and nothing for a refusal there to return out of: `exit 3 |
# exit 4` with the second stage refused answers 3, with `${PIPESTATUS[@]}`
# reading `3` alone.
#
# What decides an element of that array is whether a *process* was made for the
# stage, and two things follow. A pipeline that started nothing at all — every
# stage refused, which for a one-command pipeline means that one command — has
# nothing to publish from: it answers 0 and leaves `${PIPESTATUS[@]}` exactly as
# the previous command left it. And `shopt -s lastpipe` with job control off,
# which really does run the last stage in the owning shell, gives that stage a
# slot whether it ran or not — refused, the slot reads 0.
#
# A handler that returns 2 rather than merely non-zero asks for the enclosing
# function to return, and that costs the stage it announced like any other
# refusal *and* every stage after it, which are never even named. The stages
# already started are not undone: they finish, publish `${PIPESTATUS[@]}`
# between them, and the function returns with that shortened pipeline's own
# answer rather than with the handler's 2. Only a verdict on the first stage,
# which leaves nothing to publish, returns the 2 itself.
#
# A `&` job is announced before the fork, so a return verdict there reaches the
# shell while it still has the job in hand — and it keeps it. The stages already
# started are waited for right where they are, exactly as a foreground pipeline
# would be, and the `&` goes the way of the stages the verdict took: nothing is
# backgrounded, `$!` is never assigned, and the function leaves with the same
# answer the foreground form gives.
e() { sed 's/^.*: line [0-9]*: //'; }
p() { echo "--- $1"; ( eval "$1"; echo "rc=$?" ) 2>&1 | e; }
# Refuse the Kth announcement and say which one it was.
c() { n=$((n+1)); echo "F$n:<$BASH_COMMAND>" >&2; [ "$n" != "$K" ]; }
# Refuse announcements K through J inclusive, so a whole pipeline can go at once.
b() { n=$((n+1)); echo "F$n:<$BASH_COMMAND>" >&2; [ "$n" -lt "$K" ] || [ "$n" -gt "$J" ]; }
R='n=0; shopt -s extdebug'
# `false | false` leaves a two-element [1 1] behind, so a pipeline that
# publishes nothing is plainly distinguishable from one that publishes [0]. It
# also moves the counter by two, which every K below accounts for.
D='false | false'
E='st=$? ps="${PIPESTATUS[*]}"; trap - DEBUG; echo "st=$st ps=[$ps]"'
L='shopt -s lastpipe; set +m'

echo "=== a refused stage drops out of the pipeline and out of PIPESTATUS"
p "$R; K=99; trap c DEBUG; $D; exit 3 | exit 4 | exit 5; $E"
p "$R; K=3;  trap c DEBUG; $D; exit 3 | exit 4 | exit 5; $E"
p "$R; K=4;  trap c DEBUG; $D; exit 3 | exit 4 | exit 5; $E"

echo "=== …the last one included, which is not run by the owning shell either"
p "$R; K=5;  trap c DEBUG; $D; exit 3 | exit 4 | exit 5; $E"
p "$R; K=4;  trap c DEBUG; $D; exit 3 | exit 5; $E"

echo "=== a pipeline that started nothing at all publishes nothing"
# A lone command refused is the same rule with nothing left over, which is the
# point: there is no separate rule for the last stage, only one for a pipeline
# with no stages left in it.
p "$R; K=3;  trap c DEBUG; $D; exit 3; $E"
p "$R; K=3; J=4; trap b DEBUG; $D; exit 3 | exit 4; $E"
p "$R; K=3; J=5; trap b DEBUG; $D; exit 3 | exit 4 | exit 5; $E"
p "$R; set -o pipefail; K=3; J=5; trap b DEBUG; $D; exit 3 | exit 4 | exit 5; $E"
# And it started nothing: no stage of it runs, so nothing is waited for.
rm -f o1 o2
p "$R; K=1; J=2; trap b DEBUG; echo one > o1 | tee o2
   for f in o1 o2; do if [ -e \$f ]; then echo \"\$f=[\$(cat \$f)]\"; else echo \"\$f=-\"; fi; done"
rm -f o1 o2

echo "=== lastpipe gives the last stage a slot whether it ran or not"
# `shopt -s lastpipe` with job control off is the one arrangement in which the
# last stage really is run by the owning shell. There is then no process for the
# refusal to decline to make, so the slot is in the array regardless and reads 0
# — where without lastpipe the same refusal shortens the array instead.
p "$L; $R; K=99; trap c DEBUG; $D; exit 3 | exit 4; $E"
p "$L; $R; K=4;  trap c DEBUG; $D; exit 3 | exit 4; $E"
p "$L; $R; K=3;  trap c DEBUG; $D; exit 3 | exit 4; $E"
p "$L; $R; K=5;  trap c DEBUG; $D; exit 3 | exit 4 | exit 5; $E"
p "$L; $R; set -o pipefail; K=4; trap c DEBUG; $D; exit 3 | exit 4; $E"
p "$L; $R; set -o pipefail; K=4; trap c DEBUG; $D; exit 0 | exit 4; $E"
# …but a pipeline that started nothing still publishes nothing, lastpipe or not.
p "$L; $R; K=3; J=4; trap b DEBUG; $D; exit 3 | exit 4; $E"
# The stage does not run, so what it would have done to the shell it runs in
# does not happen either.
p "$L; $R; K=99; trap c DEBUG; echo hi | read v; trap - DEBUG; echo \"v=[\${v-unset}]\""
p "$L; $R; K=2;  trap c DEBUG; echo hi | read v; trap - DEBUG; echo \"v=[\${v-unset}]\""

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

echo "=== the stages that did run are ordinary stages, waited for as ever"
# The pipeline that remains is just a pipeline, so refusing the last stage does
# not hand control back early: `echo one > o1` has finished by the time the
# pipeline returns, exactly as if the stage had never been written.
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
# fork, so refusing one takes that stage away exactly as in the foreground.
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

# A `return` verdict needs its own shell to be measured in, because `p` is
# itself a function: a `return` that reached it would leave the *probe* rather
# than the thing under test, and every case below would collapse into the same
# blank. `z` runs the script in a fresh shell instead, where the only function
# there is to leave is the one the script defines.
z() { echo "--- $1"; "$BASH" --norc -c "$1" 2>&1 | e; echo "rc=${PIPESTATUS[0]}"; }
Y='y() { n=$((n+1)); echo "F$n:<$BASH_COMMAND>" >&2; [ "$n" != "$K" ] || return 2; }'
X='x() { n=$((n+1)); echo "F$n:<$BASH_COMMAND>" >&2; [ "$n" != "$K" ] || exit 9; }'
G='g() { exit 3 | exit 4 | exit 5; echo unreachable; }'

echo "=== a return verdict takes the rest of the pipeline, not the whole of it"
# Calling `g` costs two announcements, the call and the entry — `extdebug`
# turns `functrace` on, which is what makes a function's body announce itself —
# so with `false | false` ahead of it the three stages are announcements 5, 6
# and 7. The stages after the refused one are never announced at all, and the
# ones before it still run and still publish: the function returns with what
# they made of the pipeline, and only a refusal at the *first* stage, which
# leaves nothing published, returns the handler's own 2.
z "$R; $Y; $G; K=5; trap y DEBUG; $D; g; $E"
z "$R; $Y; $G; K=6; trap y DEBUG; $D; g; $E"
z "$R; $Y; $G; K=7; trap y DEBUG; $D; g; $E"
z "$R; $Y; $G; K=99; trap y DEBUG; $D; g; $E"
# With no function to leave, the same handler's 2 is merely a non-zero status:
# the stage it named is refused and the ones after it are announced and run as
# usual.
z "$R; $Y; K=3; trap y DEBUG; $D; exit 3 | exit 4 | exit 5; $E"
z "$R; $Y; K=5; trap y DEBUG; $D; exit 3 | exit 4 | exit 5; $E"

echo "=== a return verdict costs the & as well, and the job is waited for here"
# A `&` job's stages are announced before the fork, so the verdict lands while
# the shell still has the job in hand — and it keeps it: the stages already
# started are waited for right there, publish `${PIPESTATUS[@]}` between them,
# and the function leaves with that answer. Nothing is backgrounded, so `$!` is
# never assigned and the rest of the function never runs. The three answers are
# the same ones the foreground pipeline gives above, which is the point.
H='h() { exit 3 | exit 4 | exit 5 & j=$!; echo "bg=${j:+yes}"; wait "$j"; echo "w=$?"; }'
z "$R; $Y; $H; K=5; trap y DEBUG; $D; h; $E"
z "$R; $Y; $H; K=6; trap y DEBUG; $D; h; $E"
z "$R; $Y; $H; K=7; trap y DEBUG; $D; h; $E"
z "$R; $Y; $H; K=99; trap y DEBUG; $D; h; $E"
# One stage has nothing left over once the verdict has taken it, so there is no
# job to wait for and nothing to publish: the handler's own 2 stands.
I='i() { exit 3 & j=$!; echo "bg=${j:+yes}"; wait "$j"; echo "w=$?"; }'
z "$R; $Y; $I; K=5; trap y DEBUG; $D; i; $E"
# An `exit` verdict on a `&` stage still unwinds the whole shell, job and all.
z "$R; $X; K=4; trap x DEBUG; $D; exit 3 | exit 4 | exit 5 & wait; echo unreachable"

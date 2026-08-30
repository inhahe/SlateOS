# The shell's reader takes one *line* at a time, and a `;` or `&` written last
# on a line does not change that: it separates the command before it from the
# newline, not from whatever is written underneath. `alias foo=…;` on its own
# line is still a whole unit, and the `foo` beneath it is parsed afterwards.
#
# It is easy to get wrong, because within a line those two operators really do
# chain another command onto the same unit — so a parser that reads the
# separator and then simply looks for the next command finds it on the *next*
# line and joins the two. Everything that keys off the reader's granularity then
# shifts by a line: an alias defined on such a line is invisible to the command
# beneath it (they were parsed together, before the alias existed), a finished
# job's row is not swept between them, and `set -v` echoes both before running
# either.
#
# The counterpart is what makes it a question of the *line* rather than of the
# operator: the same two commands written on one line are one unit in every
# shell, and none of those three things happens between them.
#
# The job probes are timed as in jobs-sweep-line.sh — `sleep 0.05` against a
# `sleep 0.5` gap, so the job is certainly still alive when its row is built and
# certainly finished when anything looks at it. Those add up, and each `sleep`
# costs a process spawn, so this states a budget of its own.
# TIMEOUT: 60

echo "=== an alias defined on a \`;\`-terminated line reaches the line below"
# The names are deliberately unlikely ones: the contrast below is between an
# alias and a *missing* command, so a real command of the same name would hide
# it.
shopt -s expand_aliases
alias zzfoo='echo ALIASED';
zzfoo
# …and, written on one line, does not: the two were parsed together.
alias zzbar='echo BARRED'; zzbar
echo "  rc=$?"

echo "=== a finished job's row is swept across the boundary a \`;\` leaves"
sleep 0.05 & sleep 0.5
jobs;
jobs %1; echo "  rc=$?"
# On one line there is no boundary between them, so the row is still there.
sleep 0.05 & sleep 0.5
jobs; jobs %1; echo "  rc=$?"

echo "=== \`set -v\` echoes one line at a time, not two"
# The echo is per unit and comes *before* the unit runs, so where the boundary
# falls is legible in the order alone: two lines echoed back to back with both
# their outputs after them were one unit. This is the probe that reaches a
# trailing `&` as well, whose command runs in a subshell and so cannot say
# anything about the parent's own reader from the inside.
#
# Sourcing rather than running the file, because `.` has a read-eval loop of its
# own with the same granularity — and it keeps `set -v` from escaping into the
# rest of this case.
printf 'set -v\necho one;\necho two\n' > v1.sh
v=$( { . ./v1.sh; } 2>&1 ); echo "  v=[$v]"
# The `&` line needs something *before* the `&` to say when it ran: a
# backgrounded command's own output arrives whenever the job gets to it, which
# would decide the comparison by race rather than by boundary.
printf 'set -v\necho one; true &\necho two\nwait\n' > v2.sh
v=$( { . ./v2.sh; } 2>&1 ); echo "  amp v=[$v]"
# On one line the two really are one unit, and are echoed as one.
printf 'set -v\necho one; echo two\n' > v3.sh
v=$( { . ./v3.sh; } 2>&1 ); echo "  joined v=[$v]"

# bash's `line_number` (`parse.y:1749`) is *one* global shared by the lexer and
# the executor, and a script is read one parse unit at a time — so the counter
# the parser uses to record the *next* unit's lines is whatever the previous
# unit's execution left behind.
#
# Normally that is invisible, because each compound-command executor saves the
# counter on entry and puts it back on exit:
#
#   save_line_number = line_number;                (execute_cmd.c:2883)
#   …
#   line_number = save_line_number;                (execute_cmd.c:3003)
#
# Those are plain assignments, and a `jump_to_top_level(DISCARD)` is a `longjmp`
# straight to `reader_loop` — so the restore never runs, and the counter is left
# below where the lexer had driven it. Every line number for the rest of the
# script is low by the difference, cumulatively, and it never recovers.
#
# Two constructs escape it, because bash guards the counter with an *unwind*
# protect there, and unwind-protects do run on a `longjmp`:
#
#   unwind_protect_int (line_number);              (execute_cmd.c:5095, a call)
#   unwind_protect_int (line_number);              (builtins/evalstring.c:232)
#
# — but a function call restores the *call site*, not the unit's last line, so a
# call made from inside a compound drifts exactly as far as the compound does.
# A subshell and a command substitution escape for the other reason: bash forks
# for both, so the abort perturbs only the child's copy.
#
# Each section below prints the line it thinks it is on, so the whole case is a
# reading of the counter. The abort is `m=1+` under `declare -i`, which is the
# deeper of bash's two discards; one section uses `${a[1-]}` to show that the
# ordinary expansion-error discard drifts identically.
#
# Nothing resets the counter between sections, because there is no way to: the
# loss is permanent for the rest of the script. So each printed number carries
# every loss above it, and the case is read as a running total rather than as
# independent rows.

echo "=== a top-level abort loses nothing: it *is* the unit's last line"
declare -i m
m=1+
echo "A $LINENO"

echo "=== one enclosing compound, one line — the closing keyword"
if true; then
  m=1+
fi
echo "B $LINENO"

echo "=== and the loss is cumulative across aborts"
if true; then
  m=2+
fi
echo "C $LINENO"

echo "=== two compounds lose two"
if true; then
 while true; do
  m=3+
 done
fi
echo "D $LINENO"

echo "=== the ordinary expansion-error discard drifts the same way"
if true; then
  echo ${a[1-]}
fi
echo "E $LINENO"

echo "=== a function call restores the counter — to the call site"
g() {
  m=4+
}
g
echo "F $LINENO"

echo "=== …so a call from inside an if loses what the if loses, not more"
if true; then
  g
fi
echo "G $LINENO"

echo "=== an eval string's own compounds cost the caller nothing"
eval 'if true; then
  m=5+
fi'
echo "H $LINENO"

echo "=== but an eval written inside one still costs that one"
if true; then
  eval "m=6+"
fi
echo "I $LINENO"

echo "=== a subshell is forked, so its abort stays in the child"
if true; then
  ( m=7+ )
fi
echo "J $LINENO"

echo "=== and so is a command substitution"
if true; then
  x=$( m=8+ )
fi
echo "K $LINENO"

echo "=== the drift reaches code parsed after it, including later functions"
h() {
  echo "IN $LINENO"
}
h
echo "L $LINENO"

echo "=== and an eval or substitution body is based on bash's number, not the true one"
eval 'echo "M $LINENO"'
echo "N $(echo $LINENO)"
echo "O `echo $LINENO`"

echo "still here"

# bash's `line_number` is one register, and the *reader* is what seeds it: the
# lexer bumps it per newline consumed and never re-seeds it per command, so when
# a parse unit is handed to the executor the register is sitting on that unit's
# **last** line. Each executor that wants a line of its own assigns over it and
# puts it back — `cm_simple` with `save_line_number = line_number; …
# SET_LINE_NUMBER (Simple->line); … line_number = save_line_number;`
# (execute_cmd.c:849, 863, 867), the compound commands with the same shape
# around their stamped line (2883/3003 and friends).
#
# So anything bash does *not* stamp shows the reader's value through, and that
# is the end of the unit — not the offending command's first token:
#
#   * `check_identifier` in `execute_for_command` runs before
#     `line_number = for_command->line` (2884 vs 2897), and the same in
#     `execute_select_command` (3401 vs 3405) — so a bad loop variable is
#     blamed at the end of the unit even though the `for` is stamped;
#   * a group `{ … }`, a `while`, an `until` and an `if` carry no line at all,
#     so a redirection error on one of them is blamed there too.
#
# Two constructs *do* re-seed it, and both are visible below:
#
#   * a subshell stands on its `)`'s line — `make_subshell_command` stamps
#     `temp->line = line_number` at reduce time (make_cmd.c:824) and
#     `execute_command_internal` brackets the child with it (648, 650, 703);
#   * a function call stands on the line its body's `{` opened —
#     `line_number = function_line_number = tc->line` (execute_cmd.c:5205),
#     from `function_bstart = line_number` where the `{` was read
#     (parse.y:3271, make_cmd.c:791).
#
# Every case is written so the unit ends on a line *after* the offending
# command, which is the only way the difference shows. The redirection errors
# use `${a[1-]}` rather than `${nope?bad}` so they stay non-fatal in a
# non-interactive shell, and because an unstamped compound leaves the counter
# already at the unit's end there is no drift for the next case to inherit.

echo "=== a bad loop variable is blamed where the unit ends, not at the for"
for 'a[0]' in x; do :; done; echo \
  tail
echo "rc=$?"
select 'a[0]' in x; do :; done < /dev/null; echo \
  tail
echo "select rc=$?"

echo "=== a preceding command on the same unit does not leave its line behind"
echo two; for 'a[0]' in x; do :; done; echo \
  tail
echo "rc=$?"

echo "=== an unstamped compound's redirection is blamed the same way"
{ :
} > "${a[1-]}"; echo \
  tail
echo "rc=$?"
while
  :
do :; done > "${a[1-]}"; echo \
  tail
echo "while rc=$?"
until
  :
do :; done > "${a[1-]}"; echo \
  tail
echo "until rc=$?"
if true
then :
fi > "${a[1-]}"; echo \
  tail
echo "if rc=$?"

echo "=== an enclosing group, if or while does not re-seed it either"
{ for 'a[0]' in x; do :; done
  echo tail
}
echo "rc=$?"
if true; then
  for 'a[0]' in x; do :; done
fi; echo \
  tail
echo "rc=$?"
while :
do
  for 'a[0]' in x; do :; done
  break
done; echo \
  tail
echo "rc=$?"

echo "=== a subshell stands on its closing paren"
( for 'a[0]' in x
do :; done ); echo \
  tail
echo "rc=$?"
( for 'a[0]' in x
do :; done
); echo \
  tail
echo "rc=$?"
(
  for 'a[0]' in x; do :; done ); echo \
  tail
echo "rc=$?"

echo "=== a function body stands on the line its brace opened"
f() {
  for 'a[0]' in x; do :; done
}
echo one; f; echo \
  tail
echo "rc=$?"
g()
{
  for 'a[0]' in x; do :; done
}
g
echo "rc=$?"
h() ( for 'a[0]' in x
do :; done
)
h; echo \
  tail
echo "rc=$?"

# A simple command does carry a stamp of its own, but `make_bare_simple_command`
# (make_cmd.c:505) takes it at *reduce* time — after bison has fetched the
# lookahead that ends the first word — so a backslash-continued command is
# blamed on the continuation's line, not on the line the command word sits on.
# The body line below is the `echo`'s own; the one after the call shows both
# that the register went back and that this second `echo` was stamped past its
# continuation.
echo "=== while a simple command carries a stamp of its own, taken at reduce time"
k() {
  echo "in $LINENO"
}
k; echo \
  "after $LINENO"

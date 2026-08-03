# `shift [n]` drops the first `n` positional parameters. `n` defaults to 1, and
# the interesting part is everything around that default:
#
#   * **The count is a plain number, not an arithmetic expression.** bash reads
#     it with `legal_number`, so `shift n` and `shift 1+1` are both `numeric
#     argument required` — `shift $((1+1))` works only because the *shell*
#     expanded it before the builtin ever saw it. A leading `+`/`-` is part of
#     the number, though.
#   * **Past the end is a silent failure**: status 1, and not one parameter
#     moves. `shopt -s shift_verbose` is what gives it a message, and posix mode
#     reaches that message only *through* the option — so turning the option off
#     inside the mode is silent again.
#   * A **negative** count is `shift count out of range`, and that one is
#     reported whatever `shift_verbose` says.
#   * **A second operand is not a mere failure.** bash's `no_args` ends in
#     `jump_to_top_level(DISCARD)`, so `shift 1 2` abandons the rest of the
#     current top-level command — the `;` list, the `&&` arm, the enclosing loop,
#     the calling function — leaving `$?` at 1 while the *next* command runs. A
#     subshell is the boundary: it ends with 1 and the parent's line carries on.
#     The count is parsed first, so `shift x 2` is the ordinary numeric failure
#     and its line does continue.
#
# `$0` is not a positional parameter, so nothing here touches it, and a function
# shifts its own arguments rather than its caller's.

echo "=== it moves them down, one by default"
set -- a b c d e
shift;    echo "  shift    -> $# [$*]"
shift 2;  echo "  shift 2  -> $# [$*]"
shift 0;  echo "  shift 0  -> $# [$*] rc=$?"
shift +1; echo "  shift +1 -> $# [$*] rc=$?"

echo "=== past the end: status 1, and nothing moves"
set -- a b c
shift 9 2>&1;  echo "  shift 9  rc=$? $# [$*]"
set --
shift 2>&1;    echo "  no args  rc=$? $#"
shift 0;       echo "  shift 0  rc=$? $#"

echo "=== shift_verbose is what gives that a message"
( shopt -s shift_verbose; set -- a; shift 9 2>&1; shift 2>&1 )
( set -o posix; set -- a; shift 9 2>&1; echo "  rc=$?" )
( set -o posix; shopt -u shift_verbose; set -- a; shift 9 2>&1; echo "  silent again rc=$?" )

echo "=== a negative count is out of range, and always says so"
set -- a b c
shift -1 2>&1;    echo "  rc=$? $# [$*]"
shift -- -2 2>&1; echo "  rc=$? $# [$*]"
# `--` is stripped before the count is read, but the *out-of-range* diagnostics
# name the first word as written and so report `--` — bash raises them from
# `shift_builtin`, which still holds the unadvanced list. The "numeric argument
# required" one comes from inside `get_numeric_arg`, past the `--`, and names
# the count instead.
shift -- zz 2>&1; echo "  rc=$? $# [$*]"
( shopt -s shift_verbose; set -- a; shift -- 5 2>&1 )

echo "=== the count is a number, not an expression"
set -- a b c d
shift n 2>&1;      echo "  shift n      $# [$*] rc=$?"
shift 1+1 2>&1;    echo "  shift 1+1    $# [$*] rc=$?"
shift 0x2 2>&1;    echo "  shift 0x2    $# [$*] rc=$?"
shift '' 2>&1;     echo "  shift ''     $# [$*] rc=$?"
shift $((1+1));    echo "  shift \$((…))  $# [$*] rc=$?"

echo "=== a second operand abandons the rest of the command"
set -- a b c
shift 1 2 2>&1; echo "  NOT REACHED"
echo "  next command: rc=$? $# [$*]"
shift 1 2 2>&1 && echo "  NOT REACHED"; echo "  NOT REACHED EITHER"
echo "  after the && line: rc=$?"
for i in 1 2; do echo "  iter $i"; shift 9 9 2>&1; echo "  NOT REACHED"; done
echo "  after the loop: rc=$?"
g() { echo "  in g"; shift 1 2 2>&1; echo "  NOT REACHED"; }
g; echo "  NOT REACHED"
echo "  after g: rc=$? $# [$*]"

echo "=== but a subshell contains it"
( echo "  in sub"; shift 1 2 2>&1; echo "  NOT REACHED" ); echo "  after sub rc=$?"
v=$( shift 1 2 2>&1; echo "  NOT REACHED" ); echo "  captured=[$v] rc=$?"
shift 1 2 2>&1 | cat; echo "  after the pipe rc=$?"

echo "=== and a bad count is read first, so that line continues"
shift x 2 2>&1; echo "  rc=$? $# [$*]"

echo "=== a function shifts its own parameters"
f() { shift; echo "  in f: $# [$*]"; shift 9; echo "  past the end in f: rc=$? $#"; }
f 1 2 3
echo "  outside: $# [$*]"

echo "=== \$0 is not one of them"
set -- a b
shift 2; echo "  \$0 still set: ${0:+yes}  \$#=$#"

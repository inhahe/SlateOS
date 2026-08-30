# `declare -G` means two things — "bind globally" and "unless the frame's own
# binding answers" — and a **compound** operand receives neither of them.
#
# The command a compound operand decomposes into first
# (`expand_declaration_argument`, subst.c:12662-12745) does not read the
# command's letters at all. It spells its own option string out of the *word
# flags* `fix_assignment_words` put on the operand, and that scan is the
# narrower of the two readings:
#
#   * `W_ASSNGLOBAL` is set from a **lowercase `g`** and nothing else
#     (execute_cmd.c:4246), so an uppercase `-G` leaves it clear;
#   * `W_CHKLOCAL` is set at one place only (execute_cmd.c:4221), for an
#     assignment builtin that makes no locals — `readonly` and `export`. No
#     letter reaches it.
#
# So `declare -Ga g=(1 2)` decomposes into a plain **local** `declare -a g`,
# the literal, and only then the real `declare -Ga g` — and the two ends of it
# land in different scopes. A *scalar* operand of the very same command has no
# such first step: it is handed to the builtin whole, and takes the `-G` it was
# written with.

echo '=== the same letters, a compound operand and a scalar one'
( f() { declare -Ga g=(1 2); declare -p g; }; f; echo AFTER; declare -p g )
( f() { declare -Ga g=1; declare -p g; }; f; echo AFTER; declare -p g )

echo '=== an enclosing global is not touched by the compound one'
( g=old
  f() { declare -Ga g=(1 2); declare -p g; }; f; echo AFTER; declare -p g )
( g=old
  f() { declare -GA g=([k]=v); declare -p g; }; f; echo AFTER; declare -p g )

echo '=== the lowercase letter is the one the literal reads'
( g=old
  f() { declare -ga g=(1 2); declare -p g; }; f; echo AFTER; declare -p g )
( g=old
  f() { declare -gGa g=(1 2); declare -p g; }; f; echo AFTER; declare -p g )

echo '=== the builtin proper still has both, so its own restart still happens'
# The array stays in the frame, and the real `declare -Ga g` that follows it
# walks the global chain, finds `z` answering to nothing, and makes it — in the
# shape *its* kind letter named (declare.def:786-792).
( declare -n g=z
  f() { declare -Ga g=(1 2); declare -p g; }; f; echo AFTER; declare -p g z )
( declare -n g=z
  f() { declare -GA g=([k]=v); declare -p g; }; f; echo AFTER; declare -p g z )

echo '=== with no kind letter of its own it leaves a scalar, whatever the literal was'
( declare -n g=z
  f() { declare -G g=(1 2); declare -p g; }; f; echo AFTER; declare -p g z )

echo '=== spelled with both letters, both halves go out'
( declare -n g=z
  f() { declare -gGa g=(1 2); declare -p g; }; f; echo AFTER; declare -p g z )
( declare -n g=z
  f() { declare -Ga g=1; declare -p g; }; f; echo AFTER; declare -p g z )

echo '=== the frame keeps its own binding from `-G`, but only the builtin asks'
( declare -n g=z
  f() { local g=5; declare -Ga g=(1 2); declare -p g; }; f; echo AFTER; declare -p g z )

echo '=== a cycle counts the walks, and `-gG` and `-g` come to the same number'
# `-Ga` warns twice: the literal never walks, and the builtin proper walks the
# global table for its restart and then again through `chklocal`'s
# `find_variable` (declare.def:149). `-gGa` warns once, and leaves what `-ga`
# leaves — step 1 has the lowercase letter, so it makes the array at `g` itself
# and the builtin after it has only to find it.
( declare -n g=z; declare -n z=g
  f() { declare -Ga g=(1 2); declare -p g; }; f; echo AFTER; declare -p g z )
( declare -n g=z; declare -n z=g
  f() { declare -gGa g=(1 2); declare -p g; }; f; echo AFTER; declare -p g z )
( declare -n g=z; declare -n z=g
  f() { declare -ga g=(1 2); declare -p g; }; f; echo AFTER; declare -p g z )

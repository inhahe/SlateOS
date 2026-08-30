# `source` pushes a frame onto the *same* stack as function calls, so
# FUNCNAME/BASH_SOURCE/BASH_LINENO and `caller` interleave the two. Measured
# against bash 5.2:
#   * BASH_SOURCE[i] is a function's *definition* file, or a source frame's
#     path exactly as written on the `.` command line;
#   * BASH_LINENO[i] is the line at which frame i was entered;
#   * FUNCNAME shows source frames as the literal name `source` and carries a
#     bottom `main` — but the variable is left *unset* whenever no function
#     frame is active, even inside a sourced file the other arrays do show;
#   * subshells and command substitutions inherit the source stack, so
#     `return` stays legal inside them.
SHOW='echo "n=${#FUNCNAME[@]} F=[${FUNCNAME[@]}] set=${FUNCNAME+yes} S=[${BASH_SOURCE[@]}] L=[${BASH_LINENO[@]}]"'
printf '%s\n' "$SHOW" > f1.sh
printf 'w() {\n%s\ncaller 0\n}\n' "$SHOW" > f2.sh
printf '. ./f1.sh\n' > f3.sh
printf 'y() {\n. ./f1.sh\n}\ny\n' > f4.sh
printf 'echo pre\n( return 3 )\necho "sub=$?"\nreturn 4\n' > f5.sh

eval "$SHOW"
. ./f1.sh
eval "$SHOW"

# A function defined by a sourced file keeps naming that file forever.
. ./f2.sh
w

# A function defined here but called from a sourced file: `caller 0` sees the
# source frame as its caller.
z() {
  eval "$SHOW"
  caller 0
  caller 1
}
printf 'z\n' > f6.sh
. ./f6.sh

# source -> function -> source
. ./f4.sh

# Subshell inherits the frames; `return` in the sourced file unwinds it only.
. ./f5.sh
echo "rc=$?"

# Command substitution inherits them too.
echo "[$(. ./f1.sh)]"

# Nested source: two frames stack up.
. ./f3.sh
eval "$SHOW"

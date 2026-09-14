#!/bin/bash
# Measure GNU env's -S/--split-string grammar. NOT a gate and not a harness --
# a one-shot measurement whose output becomes the test table for our own -S.
#
# Why a script rather than a shell one-liner: every interesting case is a
# backslash escape, and the Bash tool collapses `\\` to `\` before bash sees
# it. Written to a file with the Write tool, the bytes are what they say.
#
# Each case runs GNU env with a split string whose first word is
# /usr/bin/printf and whose format is <%s>, so the output shows the argument
# BOUNDARIES that -S produced: `<a><b>` is two arguments, `<a b>` is one.
set -u

ENV=/usr/bin/env
export FOO=bar
export EMPTY=

n=0
case_of() {
    n=$((n + 1))
    local label="$1"; shift
    local out rc
    out=$("$ENV" "$@" 2>&1)
    rc=$?
    printf '%2d  %-28s rc=%-3d %s\n' "$n" "$label" "$rc" "$out"
}

echo "=== GNU $($ENV --version | head -1) ==="

# --- the control ------------------------------------------------------------
# The first version of this control was WRONG and is kept as a comment because
# the mistake is easy to repeat:
#
#     case_of 'CONTROL -S split'  -S'/usr/bin/printf <%s> a b'
#     case_of 'CONTROL no -S'     /usr/bin/printf '<%s>' a b
#
# Both print `<a><b>`. They were supposed to differ if -S were ignored, but
# they agree whether it is honoured or not, so the pair can only ever pass. A
# control that cannot fail is not a control.
#
# These two can. The first NEEDS -S to work -- without splitting, the whole
# string is one command name and there is no such file. The second is the
# same string without -S, and must therefore FAIL. Both halves are asserted,
# which is the two-probe rule: proof it runs and proof it can refuse.
case_of 'CONTROL needs -S'      -S'/usr/bin/printf <%s> a b'
case_of 'CONTROL must fail'     '/usr/bin/printf <%s> a b'

# --- splitting --------------------------------------------------------------
case_of 'single space'          -S'/usr/bin/printf <%s> a b'
case_of 'runs of spaces'        -S'/usr/bin/printf <%s> a     b'
case_of 'leading space'         -S'   /usr/bin/printf <%s> a'
case_of 'trailing space'        -S'/usr/bin/printf <%s> a   '
case_of 'tab separates'         -S'/usr/bin/printf <%s> a	b'
case_of 'empty string'          -S''

# --- escapes ----------------------------------------------------------------
case_of 'backslash-underscore'  -S'/usr/bin/printf <%s> a\_b'
case_of 'backslash-backslash'   -S'/usr/bin/printf <%s> a\\b'
case_of 'backslash-t'           -S'/usr/bin/printf <%s> a\tb'
case_of 'backslash-n'           -S'/usr/bin/printf <%s> a\nb'
case_of 'backslash-c'           -S'/usr/bin/printf <%s> a \c b'
case_of 'backslash-hash'        -S'/usr/bin/printf <%s> a\#b'
case_of 'backslash-dollar'      -S'/usr/bin/printf <%s> a\$b'
case_of 'backslash-space'       -S'/usr/bin/printf <%s> a\ b'
case_of 'backslash-unknown'     -S'/usr/bin/printf <%s> a\qb'

# --- quoting ----------------------------------------------------------------
case_of 'single quotes'         -S"/usr/bin/printf <%s> 'a b'"
case_of 'double quotes'         -S'/usr/bin/printf <%s> "a b"'
case_of 'sq inside dq'          -S'/usr/bin/printf <%s> "a '"'"'b"'
case_of 'dq inside sq'          -S"/usr/bin/printf <%s> 'a \"b'"
case_of 'unterminated sq'       -S"/usr/bin/printf <%s> 'a b"
case_of 'unterminated dq'       -S'/usr/bin/printf <%s> "a b'
case_of 'adjacent quotes join'  -S'/usr/bin/printf <%s> a"b c"d'
case_of 'escape inside sq'      -S"/usr/bin/printf <%s> 'a\\tb'"
case_of 'escape inside dq'      -S'/usr/bin/printf <%s> "a\tb"'

# --- comments ---------------------------------------------------------------
case_of 'hash mid-string'       -S'/usr/bin/printf <%s> a #b c'
case_of 'hash start of word'    -S'/usr/bin/printf <%s> a # b c'
case_of 'hash glued to word'    -S'/usr/bin/printf <%s> a#b c'

# --- variable expansion -----------------------------------------------------
case_of 'bare $VAR'             -S'/usr/bin/printf <%s> $FOO'
case_of 'braced ${VAR}'         -S'/usr/bin/printf <%s> ${FOO}'
case_of 'VAR glued'             -S'/usr/bin/printf <%s> x${FOO}y'
case_of 'unset var'             -S'/usr/bin/printf <%s> [$NOPE]'
case_of 'empty var'             -S'/usr/bin/printf <%s> [$EMPTY]'
case_of 'var splits?'           -S'/usr/bin/printf <%s> $WITHSPACE'
case_of 'var in dq'             -S'/usr/bin/printf <%s> "$FOO"'
case_of 'var in sq'             -S"/usr/bin/printf <%s> '\$FOO'"
case_of 'unclosed brace'        -S'/usr/bin/printf <%s> ${FOO'
case_of 'dollar alone'          -S'/usr/bin/printf <%s> $'

# --- the long form and attachment ------------------------------------------
case_of 'long form ='           --split-string='/usr/bin/printf <%s> a b'
case_of 'short detached'        -S '/usr/bin/printf <%s> a b'
case_of 'args after -S'         -S'/usr/bin/printf <%s> a' b
case_of 'assignment inside'     -S'X=1 /usr/bin/printf <%s> a'

# --- round 2: what round 1 left open ---------------------------------------
# Round 1 only ever tested the BARE `$VAR` form against unset/empty/spacey
# values, and that form errors before expansion is even attempted -- so it
# measured the bare-$ rule three times and the expansion rules zero times.
case_of 'braced unset'          -S'/usr/bin/printf <%s> [${NOPE}]'
case_of 'braced empty'          -S'/usr/bin/printf <%s> [${EMPTY}]'
case_of 'braced spacey splits?' -S'/usr/bin/printf <%s> ${WITHSPACE}'
case_of 'braced in dq'          -S'/usr/bin/printf <%s> "${FOO}"'
case_of 'braced spacey in dq'   -S'/usr/bin/printf <%s> "${WITHSPACE}"'
case_of 'braced in sq'          -S"/usr/bin/printf <%s> '\${FOO}'"
case_of 'empty braces'          -S'/usr/bin/printf <%s> ${}'

# `\_` split a bare word in round 1 (`a\_b` -> two args), which is not what
# "escape for space" would suggest. Whether it still splits inside quotes is
# the part that decides if it is an escape or a separator.
case_of 'bsl-underscore in dq'  -S'/usr/bin/printf <%s> "a\_b"'
case_of 'bsl-underscore in sq'  -S"/usr/bin/printf <%s> 'a\\_b'"

# `\c` ended the string in round 1. Inside quotes?
case_of 'bsl-c in dq'           -S'/usr/bin/printf <%s> "a\cb" z'
case_of 'bsl-c in sq'           -S"/usr/bin/printf <%s> 'a\\cb' z"

# `#` began a comment at the start of a word. At the very start of the string
# the first word is the COMMAND, so this asks what happens with no command.
case_of 'hash at very start'    -S'# /usr/bin/printf <%s> a'
case_of 'hash in dq'            -S'/usr/bin/printf <%s> "a # b"'

# Tabs and newlines as raw separators, and a newline inside the string.
case_of 'raw newline separates' -S'/usr/bin/printf <%s> a
b'
case_of 'bsl-t splits?'         -S'/usr/bin/printf <%s> a\tb c'

echo "=== $n cases ==="

# ===========================================================================
# MEASURED GRAMMAR -- GNU coreutils 9.4, 58 cases, 2026-09-14
# ===========================================================================
# Recorded here rather than in a separate document so the table and the thing
# that produced it cannot drift apart. Re-run this script to re-derive it.
#
# SPLITTING
#   * Separators are space, tab and newline; runs collapse; leading and
#     trailing runs are ignored. (3,4,5,6,7,57)
#   * `-S` with no argument is a getopt error, not a grammar one:
#     "option requires an argument -- 'S'", rc 125. Note `-S''` in a shell
#     yields the single argv word `-S`, so this is what the harness case
#     `run_case -S''` actually exercises. (8)
#   * Expansion does NOT re-split: ${VAR} holding "p q" is ONE argument. (46)
#
# ESCAPES (outside quotes and inside double quotes, except where noted)
#   The accepted set is CLOSED and was enumerated by round 4
#   (scripts/probe-env-split-escapes.sh), not sampled. It has to be: an
#   unknown escape is an ERROR, so every escape left out of the
#   implementation becomes a refusal GNU does not give.
#
#     \f \r \v \t \n   the control character, placed WITHIN the argument --
#                      \t does not split, unlike a raw tab (11,58)
#     \\ \" \' \# \$   that literal character
#     \_               a SPACE: splits when unquoted (9), literal inside "" (51)
#     \c               ends the string, rest dropped (13). ERROR inside "" (53):
#                      "'\c' must not appear in double-quoted -S string"
#
#   EVERYTHING else is "invalid sequence '\q' in -S", rc 125 -- including
#   `\ `, and including \a \b \e \0 \x, which are the ones worth naming
#   because they are standard C escapes that GNU's -S does NOT accept.
#   A backslash at the very end has its own message:
#       "invalid backslash at end of string in -S"
#
# QUOTING
#   * '...' is fully literal: no escapes, no expansion (25,37,49,52,54)
#   * "..." processes escapes and expansion (26,47), but rejects \c (53)
#   * Quotes concatenate with adjacent text: a"b c"d -> `ab cd` (24)
#   * An unterminated quote of either kind is an error (22,23):
#       "no terminating quote in -S string"
#
# COMMENTS
#   * `#` starts a comment ONLY at the start of a word; it runs to end of
#     string (27,28). Glued to a word it is literal: a#b -> `a#b` (29).
#     Literal inside double quotes too (56).
#   * A string that is entirely a comment leaves no command, so env prints
#     the environment -- which is env's documented no-command behaviour, not
#     a special case of -S (55).
#
# VARIABLE EXPANSION
#   * ONLY `${VARNAME}`. The bare `$VAR` form is an error, and so is a lone
#     `$`, `${` unclosed, and `${}` (30,33..36,38,39,50):
#       "only ${VARNAME} expansion is supported, error at: $FOO"
#     The text after "error at: " is the remainder of the string from the `$`
#     onward -- not just the offending token. (33 shows `$NOPE]` with the
#     bracket, 36 shows `$FOO"` with the quote.)
#   * An unset or empty variable expands to nothing, with NO error (44,45).
#     This is the one place the bare-$ rule hides a real difference: round 1
#     tested unset/empty/spacey values with `$VAR` and so measured the
#     bare-$ error three times and the expansion rules zero times.
#
# OPTIONS INSIDE THE STRING (round 3, scripts/probe-env-split-options.sh)
#   * They ARE honoured: `-S'-i cmd'` clears the environment, and
#     `-S'--unset=FOO cmd'` removes FOO. So the split words re-enter the
#     option parser; they are not plain operands. This is what decides the
#     implementation's shape.
#   * An option AFTER the command in the string is an argument to it, as
#     usual: `-S'printf [%s] -i'` prints `[-i]`.
#   * Options may precede -S (`env -i -S'...'`), and argv after the -S word
#     is appended to the command's arguments.
#   * `-S` is only an option while options are still being read:
#     `env Y=2 -S'...'` is rc 127, `-S...`: No such file or directory --
#     because Y=2 is an operand and operands end the options.
#   * A string that splits to nothing (empty, all spaces, only a comment)
#     leaves no command, so env prints the environment, rc 0.
# ===========================================================================

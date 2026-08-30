# A `${ … }` is scanned before any of it is expanded — see
# a-brace-sub-word-is-expanded-on-its-own-string.sh for that scan and
# a-brace-scan-carries-on-past-a-failed-extent-read.sh for what it does when one
# of its reads fails. This file pins a *second* read, of the operand alone, that
# happens only when the operand is going to be used.
#
# `parameter_brace_expand_rhs` opens with it:
#
#     if ((quoted & (Q_HERE_DOCUMENT|Q_DOUBLE_QUOTES)) && *value)
#       { sindex = 0;
#         temp = string_extract_double_quoted (value, &sindex, SX_STRIPDQ); }
#     else
#       temp = value;                              /* subst.c:7724-7732 */
#
# — a scan, not an expansion: it reads the extent of every `$( … )` it meets and
# runs none of them. So a used operand costs one diagnostic per failing
# substitution in it *over and above* the whole-word scan and the expansion that
# follows, and the same operand left unused costs only the first.
#
# `parameter_brace_expand_rhs` is reached by exactly three operators — `-`, `=`
# and `+` — and only after whatever refusal decides there is no destination.
#
# The rules are the double-quoted string's rather than the brace's: a `` ` `` is
# a byte hunt, a `$(` (including a `$((`) is read, a `${` is handed to the brace
# scan, and every other byte is copied.
#
# Only a prompt expansion suppresses the jump a failed read raises, so `${x@P}`
# is again the only place any of this can be counted.
#
# Verified against bash 5.2.37.

p() { printf '%s\n' "--- $1"; v="$2"; printf '[%s]\n' "${v@P}"; echo "  rc=$?"; }

echo "=== 1. a used operand is read once more than an unused one"
# `:=` and `=` *assign*, so every probe sets the ground it stands on.
# Used: the scan, this second read, then the expansion — three.
unset z; p ':- unset'  'A${z:-p$(fi
q)r}B'
unset z; p '- unset'   'A${z-p$(fi
q)r}B'
unset z; p ':= unset'  'A${z:=p$(fi
q)r}B'
unset z; p '= unset'   'A${z=p$(fi
q)r}B'
# Unused: the whole-word scan and nothing else — one.
unset z; p ':+ unset'  'A${z:+p$(fi
q)r}B'
z=ZZ;    p ':- set'    'A${z:-p$(fi
q)r}B'
z=ZZ;    p ':= set'    'A${z:=p$(fi
q)r}B'
# …and `+` the other way round, used exactly when the others are not.
z=ZZ;    p ':+ set'    'A${z:+p$(fi
q)r}B'
z=ZZ;    p '+ set'     'A${z+p$(fi
q)r}B'

echo "=== 2. the operators that never reach it"
# `:?` expands its operand but through `parameter_brace_expand_error`, and every
# pattern operator through a reader of its own — two reads each, not three.
unset z
p ':? unset'  'A${z:?p$(fi
q)r}B'
z=ZZ
p '# set'     'A${z#p$(fi
q)r}B'
p '% set'     'A${z%p$(fi
q)r}B'
p '/ set'     'A${z/p$(fi
q)r}B'
p '/repl set' 'A${z/x/p$(fi
q)r}B'
p ':off set'  'A${z:p$(fi
q)r}B'

echo "=== 3. it is the same kind of scan, so it carries on and reads the same rows"
unset z
# Two failing substitutions in the operand are two extra reports, not one.
p 'two subs'  'A${z:-p$(fi
q)$(for
s)r}B'
# A backquote is a byte hunt in this scan as in the other, so it cannot fail.
p 'backtick'  'A${z:-p`for
s`r}B'
# A `$((` is the same `$(` row, and its count cannot fail on a syntax error.
p 'arith'     'A${z:-p$((1+))r}B'
# A `$` that starts nothing is not the row at all. (An escaped `\$` would say
# the same, but it is also a *prompt* escape and renders by `$EUID`, which osh
# reports as root by design — see open-questions Q28.)
p 'bare dollar' 'A${z:-p$}B'
# A nested `${ … }` is handed to the brace scan, which then does its own second
# read of *its* operand: two extra, not one.
p 'nested'    'A${z:-p${w:-$(fi
q)}r}B'

echo "=== 4. the read is a read: nothing in the operand runs twice"
unset z
p 'side effect' 'A${z:-p$(echo S >&2)q}B'
unset z
p 'ok then bad' 'A${z:-p$(echo S >&2)$(fi
q)r}B'

echo "=== 5. a refusal that comes first leaves no second read"
# There are no positionals, so `${1=…}` has nowhere to put a default and bash
# says so before `parameter_brace_expand_rhs` is called: one report, not two.
# Written out rather than run through `p`, whose own `$1` would make the
# parameter active and take the other branch entirely.
set --
echo '--- $1 ='
v='A${1=p$(fi
q)r}B'; printf '[%s]\n' "${v@P}"; echo "  rc=$?"
echo '--- $@ ='
v='A${@=p$(fi
q)r}B'; printf '[%s]\n' "${v@P}"; echo "  rc=$?"

echo done

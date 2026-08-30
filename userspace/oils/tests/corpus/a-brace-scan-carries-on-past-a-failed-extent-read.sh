# A `${ … }` is scanned before any of it is expanded, and the scan reads the
# *extent* of every `$( … )` it meets — see
# a-brace-sub-word-is-expanded-on-its-own-string.sh for the scan itself. This
# file pins what the scan does when one of those reads fails.
#
# It carries on. `extract_dollar_brace_string` hands the `$(` to
# `extract_command_subst` and then simply advances:
#
#     if (string[i] == '$' && string[i+1] == LPAREN)
#       { si = i + 2; t = extract_command_subst (string, &si, flags|SX_NOALLOC);
#         i = si + 1; continue; }              /* subst.c:1896-1903 */
#
# — there is no test of how the read came out, so a read that failed leaves `si`
# where its reader stopped and the loop looks for the next `$(` from one past
# it. Every failing substitution in the word is therefore reported, not just the
# first, and the brace still finds the `}` it was counting towards.
#
# What is scanned on is *text*, which is what makes the resume point visible:
# a `$(` the failed read swallowed is not reached again, and one nested inside
# the failed body but past the point its reader stopped is.
#
# Only a prompt expansion suppresses the jump a failed read raises, so `${x@P}`
# is the only place any of this can be seen — and the operand is left unused
# throughout (`z` is set, and the operator is `:-`) so that the scan is the only
# reader. An operand that is *used* is read twice more; that is a separate rule.
#
# Verified against bash 5.2.37.

p() { printf '%s\n' "--- $1"; v="$2"; printf '[%s]\n' "${v@P}"; echo "  rc=$?"; }
z=ZZ

echo "=== 1. every failing substitution in the word is reported"
p 'one' 'A${z:-p$(fi
q)r}B'
p 'two' 'A${z:-p$(fi
q)r$(for
s)t}B'
p 'three' 'A${z:-$(fi
1)$(for
2)$(done
3)}B'
# A read that succeeds is stepped over and reports nothing — and, this being a
# scan, is not run either.
p 'fail then ok' 'A${z:-p$(fi
q)r$(echo N)t}B'
p 'ok then fail' 'A${z:-p$(echo N) $(fi
q)r}B'

echo "=== 2. the scan resumes where the read stopped, in the text"
# The reader gives up at the end of its own first line, so a `$(` on that line
# is inside what it took — the error token shows it — and is not seen again.
p 'second $( swallowed' 'A${z:-p$(fi $(for
x)q)r}B'
# One nested in the failed body but on a later line is past the stop point, and
# is read even though no parser ever took that body apart.
p 'second $( nested, past the stop' 'A${z:-p$(fi
$(for
x)q)r}B'
# Three deep, each one line further in.
p 'three nested' 'A${z:-$(fi
$(for
$(done
x)y)z)}B'

echo "=== 3. what the resumed scan does and does not read"
# A `$[ … ]` is not the scan's `$(` row, and a backquote is a byte hunt for the
# closer rather than a parse — neither can fail here.
p 'backtick after' 'A${z:-p$(fi
q)r`for
s`t}B'
p 'arith after' 'A${z:-p$(fi
q)r$(( 1 + ))t}B'
# A single-quoted run is walked over by `skip_single_quoted`, so the scan never
# reads what is in it — before the failure or after it.
p 'quoted after' 'A${z:-p$(fi
q)r'"'"'$(for
s)'"'"'t}B'
# A nested brace is just more text to the same single pass.
p 'nested brace after' 'A${z:-p$(fi
q)r${w:-$(for
s)}t}B'
p 'nested brace before' 'A${z:-p${w:-$(fi
q)}r$(for
s)t}B'

echo "=== 4. the brace still closes"
# The scan's business is finding the `}`, and a failed read does not stop it:
# every case above expands to `AZZB`, the value the brace was going to give. The
# operand is still the unused one here — an operand actually *used* is read
# twice more, which is a rule of its own.
unset z
p 'unset :+, still unused' 'A${z:+p$(fi
q)r$(for
s)t}B'
p 'set :+, used' 'A${z:+ok}B'
z=ZZ
p 'set #, operand unused' 'A${z#p$(fi
q)r$(for
s)t}B'

echo done

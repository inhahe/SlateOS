# The third piece of `parameter_brace_expand_rhs`'s extra read (see
# a-used-brace-operand-is-scanned-once-more-before-it-expands.sh for the reports
# it makes and a-used-brace-operands-embedded-quotes-lose-their-backslashes.sh
# for the byte rules it applies): what the scan puts in `temp` when the `$( … )`
# it meets will not read.
#
#     si = i + 2;
#     ret = extract_command_subst (string, &si, …);
#     temp[j++] = '$';  temp[j++] = string[i + 1];
#     for (t = 0; ret[t]; t++, j++) temp[j] = ret[t];
#     temp[j] = string[si];
#     …
#     else if (string[si]) { j++; i = si + 1; }    /* subst.c:955-993 */
#
# `xparse_dolparen` hands back the text its reader got through, less its last
# byte, and leaves `si` on that byte:
#
#     nc = ep - ostring;  *indp = ep - base - 1;
#     ret = substring (ostring, 0, nc - 1);        /* parse.y:4348-4376 */
#
# So `$` + `string[i+1]` + `ret` + `string[si]` is the original text from the
# `$(` through `si`, byte for byte — a failed read copies its region exactly as
# a successful one does, only over a shorter region. What matters is what comes
# *after*: the scan resumes at `si + 1` with its ordinary rules, `dquote` and
# all. It does not treat the remainder as opaque.
#
# That is visible because the resumed scan is still the `SX_STRIPDQ` one: the
# `"` that closes the operand's embedded run is still taken out, and a `\`
# after the failure is still judged by which side of that `"` it fell on.
#
# Every probe goes through `${u@P}`, because that is the one caller that raises
# `no_longjmp_on_fatal_error` — anywhere else a `$( … )` that will not parse
# ends the script instead of carrying on.
#
# Verified against bash 5.2.37.

p() { u="$2"; printf '%s [%s]\n' "$1" "${u@P}"; }
unset z
y=YY

echo "=== 1. the region is copied and the rest of the operand goes on"
# The read got through `fi`, so `ret` is `f` and `si` is on the `i`. What the
# expansion then meets is `p$(fi⏎q)r` — and *its* read fails the same way, runs
# the `f` it was handed (`f: command not found`) and expands `⏎q)r` after it.
p 'q only'   'A${z:-p"$(fi
q)"r}B'
p 'q around' 'A${z:-p"a$(fi
q)b"r}B'
p 'bare'     'A${z:-"$(fi
q)"}B'
# With no embedded quotes there is nothing for the scan to take out, so this is
# the same answer for the plainer reason.
p 'outside'  'A${z:-p$(fi
q)r}B'

echo "=== 2. and it goes on with its dquote rules, not as opaque text"
# `\x` after the failure is still inside the operand's own `" … "`.
p 'bs after'  'A${z:-p"$(fi
q)\x"r}B'
p 'bs before' 'A${z:-p"\x$(fi
q)"r}B'
# The `"` after the failure closes that run, so `\y` is outside one and keeps
# its backslash where `\z`, inside the next run, does not.
p 'reopen'    'A${z:-p"$(fi
q)"\y"\z"r}B'
# Inside the extent nothing is judged at all — the `\x` is part of what the
# read was handed and goes to the child as it stands.
p 'bs inside' 'A${z:-p"$(fi \x
q)"r}B'

echo "=== 3. each read is its own"
p 'two subs'  'A${z:-p"$(fi
q)"m"$(fi
n)"r}B'
# A nested `${ … }` is copied by its own extent and applies the rules itself.
p 'brace in'  'A${z:-p"${w:-W"\x"V}$(fi
q)"r}B'

echo "=== 4. the same operators as the rest of the read"
p 'op ='      'A${z=p"$(fi
q)"r}B'
# `z` is set by the row above, so this `:-` never uses its operand: the brace
# scan still reads it once (one report), the rhs read never happens (no second
# and third), and what prints is what the `=` assigned.
p 'now set'   'A${z:-p"$(fi
q)"r}B'
unset z
p 'op +'      'A${y+p"$(fi
q)"r}B'

echo done

# `$'…'` is not a single-quoted run. A `\` in one covers the next character —
# `\'` included — so the run does not end there, and a scan that copies a body
# as raw text has to read it with that rule or the body stops balancing.
#
# bash reads it with `P_ALLOWESC` from the `$'…' inside group` case of
# `parse_matched_pair` (parse.y:3847), and that case is reached for *every*
# grouping construct — `$( … )`, `$(( … ))`, `$[ … ]`, `${ … }`, a subscript —
# gated on neither `P_COMMAND` nor `P_ARITH`. Read as a plain `'…'` instead, the
# run would end at the `\` and the `'` after it would open another that never
# closes, so the whole construct fails to lex.
#
# Verified against bash 5.2.37.

q="za'bz"
s=zaaaz
declare -A h=([k]=V ["a'b"]=W)
a=(A B C)
p() { printf '%-34s -> [%s]\n' "$1" "$2"; }
e() { ( eval "$1" ) 2>&1; echo "  rc=$?"; }

echo "=== an escaped quote keeps the run open, in every construct ==="
p 'command substitution' "$(echo $'a\'b')"
p 'backticks'            "`echo $'a\'b'`"
p 'arithmetic'           "$(( ${#q} + $(echo $'\x30') ))"
p 'brace, operand'       "${q/$'a\'b'/Y}"
p 'brace, replacement'   "${q/za/$'a\'b'}"
p 'brace, trim'          "${q#$'za\'b'}"
p 'subscript'            "${a[$(echo $'\x31')]}"
p 'nested in a nested'   "$(echo $(echo $'a\'b'))"

echo "=== …and it is the *whole* run, so a delimiter inside it is protected ==="
p 'slash'                "${s/$'a\/b'aaa/Y}"
p 'brace'                "${q/$'a\'b'/{}}"

echo "=== an ordinary escape travels unresolved to the later lex ==="
# The scan wants the extent and nothing else: `\x7a` is still four characters
# when the body is copied, and becomes a `z` where the run is finally read.
p 'hex, in a body'       "${q/$'\x7a'/Y}"
p 'hex'                  "$(echo $'\x41\x42')"
p 'octal'                "$(echo $'\101\102')"
p 'escaped backslash'    "$(echo $'a\\b')"

echo "=== a plain '…' in the same place honours no escape ==="
# Its `\` is an ordinary character and its first `'` is the close — so the same
# text is a different pattern, which is why the two cannot share a reader.
p 'plain quotes'         "${q/'a\'/Y}"
p 'ansi-c quotes'        "${q/$'a\'b'/Y}"
p 'plain, backslash kept' "$(echo 'a\b')"

echo "=== an unterminated run is still an error, and names the quote ==="
e 'echo "$(echo $'"'"'a\'"'"')"'

echo done

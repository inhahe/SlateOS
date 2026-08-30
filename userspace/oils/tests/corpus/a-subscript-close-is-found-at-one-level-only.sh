# The `]` that ends a `${name[sub]}` subscript is the first one *at the
# subscript's own level*. A `]` inside a nested `$( … )`, `${ … }`, `` ` … ` ``,
# `$(( … ))` or a quoted run belongs to that construct, and ends nothing.
#
# bash never has to look: `parse_matched_pair` recorded where every nested
# construct ended while it read the body, so a `]` inside one was never a
# candidate for the close. A parser that keeps the body as raw text has to
# recover that structure before it splits — otherwise the subscript is truncated
# mid-construct and what is left no longer lexes.
#
# The distinction shows up in the *kind* of failure. Kept whole, the subscript
# `$(echo 1])` expands to `1]` and then fails as arithmetic — a runtime error,
# rc=1. Split at the inner `]`, it is an unterminated substitution and fails to
# lex instead — a parse error, rc=2. So every arithmetic complaint below is
# evidence that the construct was *not* split.
#
# Verified against bash 5.2.37.

a=(A B C D E F)
declare -A h=([k]=V ['a]b']=W)
i=2
p() { printf '%-32s -> [%s]\n' "$1" "$2"; }
e() { ( eval "$1" ) 2>&1; echo "  rc=$?"; }

echo "=== a ] inside a construct is not the close ==="
p 'command substitution' "${a[$(echo 2; : ])]}"
p 'brace expansion'      "${a[${i}]}"
p 'backticks'            "${a[`echo 3; : ]`]}"
p 'arithmetic'           "${a[$((1 + 1))]}"
p 'single quotes'        "${h['a]b']}"
p 'double quotes'        "${h["a]b"]}"
# A `$'…'` holding a `]` is deliberately *not* exercised here, and the reason is
# not this scan: bash *translates* `$'…'` in a `${ … }` body as it reads it
# (parse.y:3854) and then decides whether to put single quotes back around the
# result. In a subscript — `dolbrace_state == DOLBRACE_PARAM` — inside double
# quotes it does not, because that branch is `#if 0 /* TAG:bash-5.3 */`
# (parse.y:3875), so `"${h[$'a]b']}"` becomes `${h[a]b]}` and is a bad
# substitution *before* any `]` is looked for. Unquoted, or after an operator,
# the quotes do go back. See known-issues
# TD-OILS-AN-ANSI-C-STRING-IS-NOT-REQUOTED-AFTER-A-BRACE-BODY-TRANSLATES-IT.
p 'ansi-c, no bracket'   "${h[$'a\tb']}"
# The `]` here is inside the substitution's own quoted argument, two levels
# down, and is removed before the subscript is ever evaluated.
p 'quoted, two deep'     "${a[$(echo "2]" | tr -d "]")]:-none}"

echo "=== …so the whole construct is the subscript, and fails as arithmetic ==="
e 'a=(A B C); echo "${a[$(echo 1])]}"'
e 'a=(A B C); echo "${a[`echo 1]`]}"'
e 'a=(A B C); echo "${a[${x:-1]}]}"'
e 'a=(A B C); echo "${a[$(echo "]2")]:-none}"'
e 'a=(A B C); echo "${a["2]"]:-none}"'
e 'a=(A B C); echo "${a[2\]]:-none}"'

echo "=== the same when the subscript is only part of the body ==="
p 'length'               "${#a[$(echo 2)]}"
p 'trim'                 "${a[$(echo 2)]#C}"
p 'replace'              "${a[$(echo 2)]/C/Z}"
p 'default'              "${a[$(echo 9)]:-none}"

echo "=== nesting inside the construct still closes correctly ==="
p 'nested substitution'  "${a[$(echo $(echo 2))]}"
p 'nested brace'         "${a[${x:-${i}}]}"
p 'brace in substitution' "${a[$(echo ${i})]}"

echo "=== an unterminated construct is still the later lex's error ==="
e 'a=(A B C); echo ${a[$(echo 1]}'

echo done

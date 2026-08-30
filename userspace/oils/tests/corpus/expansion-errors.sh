# Word-expansion errors come in two very different flavours, and confusing them
# is a script-truncation bug rather than a cosmetic one:
#
#   * a handful are *fatal* — the whole (sub)shell exits: `set -u` on an unset
#     parameter, `${var:?msg}`, and a bad substitution;
#   * the rest merely *discard* — the command never runs, `$?` carries the
#     error's status, and the rest of that command's **parse unit** (everything
#     up to the next newline) is abandoned. The next line still runs.
#
# This case pins the discarding kind: each probe is followed by a line that must
# appear, so a shell that treats one of them as fatal fails loudly.

a=(1 2 3)

# A negative subscript that underflows past 0 on the *write* path. The
# diagnostic names the full reference with the raw subscript source.
a[-9]=v; echo "unreachable-1"
echo "after-write=$? n=${#a[@]}"

# Same underflow computed from an expression and from a variable — the message
# keeps the pre-arithmetic source text, not the evaluated index.
a[1+2-20]=v; echo "unreachable-2"
echo "after-expr=$?"
i=-9
a[i]=v; echo "unreachable-3"
echo "after-var=$?"

# The *read* path is softer: a value read names only the base and expands empty
# without discarding anything.
echo "read=[${a[-9]}] st=$?"

# …but the *length* form is a discarding error, and names the raw subscript
# followed by a stray `]`.
echo "len=${#a[-9]}"; echo "unreachable-4"
echo "after-len=$?"

# A positive out-of-range read is simply empty — no error at all.
echo "oor=[${a[99]}] st=$?"

# Indirect expansion through a value that is not a valid variable name.
ptr='not a name'
echo "ind=${!ptr}"; echo "unreachable-5"
echo "after-ind=$?"

# `${arr[@]=v}` would have to write to the subscript `@`, which an *indexed*
# array has no room for. Note the status: 2, not the 1 every other bad-subscript
# site uses.
unset arr
echo "atassign=${arr[@]=y}"; echo "unreachable-6"
echo "after-atassign=$?"
declare -a decl
echo "atassign2=${decl[*]:=y}"; echo "unreachable-7"
echo "after-atassign2=$?"

# An *associative* array has no such problem: its subscripts are string keys, so
# `@` and `*` are ordinary keys and the assignment simply happens.
declare -A m
echo "assoc-at=${m[@]:=z}"
declare -p m
declare -A m2
echo "assoc-star=${m2[*]=s}"
declare -p m2
# A non-empty associative array is "active", so `:=` assigns nothing.
declare -A m3=([k]=v)
echo "assoc-live=${m3[@]:=z}"
declare -p m3

# An empty subscript has no representation in either kind of array.
declare -A e=([k]=v)
e['']=x; echo "unreachable-8"
echo "after-empty=$? n=${#e[@]}"

# A malformed arithmetic subscript is an ordinary arithmetic syntax error, and
# discards the same way.
a[x y]=v; echo "unreachable-9"
echo "after-arith=$?"

# So does a plain `$(( ))` syntax error…
echo "$(( 1 + ))"; echo "unreachable-10"
echo "after-dollar-arith=$?"
# …and a negative substring offset written without a space.
s=abcdef
echo "${s: -2}"          # this one is *valid* — the space is what makes it so
echo "sub=${s:0:-99}"; echo "unreachable-11"
echo "after-substr=$?"

# As a `declare`/`local` operand the bad subscript is softer still: it fails
# only the builtin, so the rest of the very same line keeps going.
declare a[-9]=v; echo "decl-same-line=$?"
echo "after-decl=$? n=${#a[@]}"
# A malformed arithmetic *value* under `-i` is not softened — it discards.
declare -i b=1+*2; echo "unreachable-12"
echo "after-decl-int=$?"

# Everything above ran in the top-level shell, which is still alive:
echo "alive"

# The fatal kind, confined to a subshell so the diff can see what follows.
( set -u; echo "u=${undefined_zz}"; echo "unreachable-13" )
echo "nounset-sub=$?"
( echo "q=${undefined_zz:?boom}"; echo "unreachable-14" )
echo "colonq-sub=$?"
# `${var:?}` with no message uses bash's own wording, and the colon-less form
# has a different default than the colon form.
( echo "${undefined_zz:?}" )
echo "colonq-empty=$?"
( echo "${undefined_zz?}" )
echo "q-empty=$?"

echo "done"

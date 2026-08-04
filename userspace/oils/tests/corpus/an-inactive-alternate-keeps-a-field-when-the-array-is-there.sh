# `:+` is the only one of the four whose own answer can be null — `:-`, `:=` and
# `:?` all substitute a word, raise an error, or hand back the elements — and it
# is the one place where "no field" and "one empty field" part company. Quoted,
# `"${a[@]:+A}"` on `a=("")` is one empty field, while on `a=()`, or on a name
# that was never declared, it is no field at all: a reference that reached
# something keeps a null, one that reached nothing keeps nothing.
#
# Unquoted the two are the same word either way, since an empty field is
# removed, and the `[*]` spelling is one field by definition. So it takes a
# quoted `[@]` and a field count to see it.

a0=(); a1=(""); a2=("" "")
declare -A m0=(); declare -A m1=([k]="")
w=; v=x

show() { printf '  %-16s(%d)' "$1" $(($# - 1)); shift; printf '<%s>' "$@"; printf '\n'; }

echo "### quoted, where the count is the answer"
show 'a0[@]:+A'   "${a0[@]:+A}"
show 'a1[@]:+A'   "${a1[@]:+A}"
show 'a2[@]:+A'   "${a2[@]:+A}"
show 'nope[@]:+A' "${nope[@]:+A}"
show 'm0[@]:+A'   "${m0[@]:+A}"
show 'm1[@]:+A'   "${m1[@]:+A}"
show 'a0[*]:+A'   "${a0[*]:+A}"
show 'a1[*]:+A'   "${a1[*]:+A}"

echo "### unquoted, where an empty field goes away regardless"
show 'a0[@]:+A'   ${a0[@]:+A}
show 'a1[@]:+A'   ${a1[@]:+A}
show 'm1[@]:+A'   ${m1[@]:+A}

echo "### the colon-less form asks a different question, same two answers"
show 'a0[@]+A'    "${a0[@]+A}"
show 'a1[@]+A'    "${a1[@]+A}"
show 'nope[@]+A'  "${nope[@]+A}"
show 'm0[@]+A'    "${m0[@]+A}"

echo "### the other three always have something to say"
show 'a0[@]:-d'   "${a0[@]:-d}"
show 'a1[@]:-d'   "${a1[@]:-d}"
show 'a0[@]:-'    "${a0[@]:-}"
show 'a1[@]:-'    "${a1[@]:-}"
show 'nope[@]:-'  "${nope[@]:-}"
show 'a0[@]-'     "${a0[@]-}"

echo "### and a scalar keeps its field either way"
show 'w:+A'       "${w:+A}"
show 'nope:+A'    "${nope:+A}"
show 'v:+A'       "${v:+A}"

echo "### joined into a word the difference disappears"
show 'x a1 y'     "x${a1[@]:+A}y"
show 'x a0 y'     "x${a0[@]:+A}y"
show 'a1 then a0' "${a1[@]:+A}" "${a0[@]:+A}"

echo "### which is why it is counted, not printed"
set -- "${a1[@]:+A}"; echo "  a1 \$#=$#"
set -- "${a0[@]:+A}"; echo "  a0 \$#=$#"
set -- "${a1[@]:+A}" tail; echo "  a1+tail \$#=$# last=$2"

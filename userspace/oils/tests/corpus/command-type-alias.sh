# Command resolution: the builtin/function/alias precedence order, and the
# `command`/`builtin`/`type` escapes from it.

# A function shadows a builtin of the same name; `builtin` reaches past it.
echo() { command echo "wrapped:$@"; }
echo direct
builtin echo bypassed
unset -f echo
echo restored

# `command` skips functions but still finds builtins.
cd() { builtin echo "cd-function:$1"; }
cd somewhere
command cd . && builtin echo "command-cd-status=$?"
unset -f cd

# `type` classifies each kind. Only the classification word is compared here —
# the path of an external command is host-specific.
f() { :; }
alias al='echo aliased'
type -t f
type -t alias
type -t if
type -t type
type -t cd
type -t nosuchthing_xyz; echo "type-missing-status=$?"

# In a non-interactive shell aliases are NOT expanded unless expand_aliases is
# set — one of the classic script-vs-prompt differences.
al 2>/dev/null; echo "alias-unexpanded-status=$?"
shopt -s expand_aliases
alias hi='builtin echo hello-from-alias'
hi
# An alias whose value ends in a space makes the *next* word alias-expandable.
alias pre='hi '
alias post='builtin echo post-ran'
pre post
# An alias defined and used on the *same* logical line is not expanded: the whole
# line is read and parsed before any of it runs.
alias same='builtin echo same-line'; same 2>/dev/null
builtin echo "same-line-status=$?"
# `shopt -u` mid-script stops expansion again for the lines that follow.
shopt -u expand_aliases
hi 2>/dev/null; builtin echo "after-unset-shopt-status=$?"

# `command -v` names what would run; the shape (not the path) is what matters.
command -v f
command -v while > /dev/null; builtin echo "command-v-keyword-status=$?"
command -v nosuchthing_xyz > /dev/null; builtin echo "command-v-missing-status=$?"
command -V while
command -V echo

# An alias is only *reported* by `type`/`command -v` while expand_aliases is in
# effect — with the option off the name would not reach the alias, so bash calls
# it not-found. The alias also outranks a function and a builtin of the same name.
type -t al; echo "type-t-alias-off-status=$?"
shopt -s expand_aliases
# The function must be defined *before* the alias: once `shadow` is an alias, the
# `shadow()` definition line expands and is a syntax error (in bash too).
shadow() { builtin echo funcshadow; }
alias shadow='builtin echo shadowed'
type -t al
type al
type -t shadow
type -a shadow
command -v al
command -V al
type -t nosuchthing_xyz > /dev/null; echo "type-missing-again=$?"
unset -f shadow
unalias shadow
shopt -u expand_aliases
type -t al; echo "type-t-alias-off-again=$?"

# A function may recurse and see its own name in FUNCNAME.
rec() {
  builtin echo "depth=$1 name=${FUNCNAME[0]} caller=${FUNCNAME[1]-none}"
  [ "$1" -ge 2 ] && return 0
  rec $(( $1 + 1 ))
}
rec 1

# `unset -f` removes a function, `unset -v` a variable of the same name.
both=varvalue
both() { builtin echo funcvalue; }
both; builtin echo "var=$both"
unset -f both
builtin echo "after-unset-f var=$both"

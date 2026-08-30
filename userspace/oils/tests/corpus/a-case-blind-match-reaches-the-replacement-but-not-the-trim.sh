# `shopt -s nocasematch` does not reach every pattern the shell matches, and
# which ones it reaches is not guessable from the name.
#
# It reaches `case`, `[[ == ]]`, `[[ =~ ]]` and — the surprise — the parameter
# *replacement* operator `${v/pat/rep}` and its three variants. It does **not**
# reach the trim operators `${v#pat}` / `${v%pat}`, which stay case-sensitive
# with the option set, nor pathname expansion (that is `nocaseglob`, a separate
# option).
#
# So on `abcABC` with the option set, `${v/ABC/X}` finds the *first* run
# case-blindly and gives `XABC`, while `${v#ABC}` finds nothing and gives the
# value back unchanged.

p() { printf '  [%s]' "$@"; echo; }
v=abcABC

echo "=== off: every pattern is case-sensitive"
p "${v#ABC}" "${v#abc}" "${v%abc}" "${v%ABC}"
p "${v/ABC/X}" "${v//ABC/X}" "${v/#ABC/X}" "${v/%abc/X}"
case ABC in abc) echo "  case yes";; *) echo "  case no";; esac
[[ ABC == abc ]]; echo "  [[ == st=$?"
[[ ABC =~ ^abc$ ]]; echo "  [[ =~ st=$?"

shopt -s nocasematch
echo "=== on: the replacement folds, the trim does not"
p "${v#ABC}" "${v#abc}" "${v%abc}" "${v%ABC}"
p "${v##A*}" "${v%%*C}"
p "${v/ABC/X}" "${v//ABC/X}" "${v/#ABC/X}" "${v/%abc/X}"
p "${v/aBc/X}" "${v//[A-C]/.}" "${v//[a-c]/.}"
case ABC in abc) echo "  case yes";; *) echo "  case no";; esac
[[ ABC == abc ]]; echo "  [[ == st=$?"
[[ ABC =~ ^abc$ ]]; echo "  [[ =~ st=$?"

echo "=== the result keeps the original characters, only the match folds"
w=HeLLo
p "${w/hello/X}" "${w/ll/[&]}" "${w//L/-}" "${w//l/-}"

echo "=== an anchored replacement, and the longest match at the anchor"
u=AAbbAA
p "${u/#aa/X}" "${u/%aa/X}" "${u/#a*/X}" "${u/%*a/X}"

echo "=== elementwise over an array and over the positional parameters"
A=(aB Cd EEff)
p "${A[@]/AB/X}" "${A[@]//[CE]/-}" "${A[*]/#c/K}"
set -- One TWO three
p "${@/one/1}" "${*//O/0}"

echo "=== extglob patterns fold too"
shopt -s extglob
p "${v/@(ABC|zzz)/X}" "${v//+([A-C])/-}" "${v/!(z)/Y}"
shopt -u extglob

echo "=== and it goes back off"
shopt -u nocasematch
p "${v/ABC/X}" "${w/hello/X}"
case ABC in abc) echo "  case yes";; *) echo "  case no";; esac

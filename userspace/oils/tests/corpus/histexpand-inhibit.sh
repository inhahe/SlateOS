# History expansion is normally an interactive-only transformation, but a script
# that turns both switches on gets it too — which is what lets this case pin
# bash's `bash_history_inhibit_expansion` rules differentially. `!` has five
# meanings in shell syntax and history expansion has to stand aside for four of
# them; the interesting part is exactly *where* the line is drawn.
set -o history
set -H

# --- `$!`: the last background pid, so never an event ---------------------
echo "[$!]"
echo "[a$!b]"
echo "[$$!q]" | sed 's/[0-9][0-9]*/PID/'

# --- `${!name}`: indirect expansion --------------------------------------
target=found
ref=target
echo "${!ref}"
pre_one=1
pre_two=2
echo ${!pre_*}
arr=(x y z)
echo "${!arr[@]}"
declare -A m=([k]=v)
echo "${!m[@]}"

# The inhibition is only for a `!` directly after the `${`, and only when the
# `}` is on the line. Each of these is therefore a *real* event reference that
# fails — and a failed expansion abandons the line without running it, so the
# `echo` never runs. It does not disturb `$?` either, which is why every `rc=`
# below reports the status of the *previous* successful command.
echo before-1
echo ${x#!}
echo "rc=$?"
echo ${x:-!q}
echo "rc=$?"
echo ${a!b}
echo "rc=$?"
echo ${ !q}
echo "rc=$?"

# --- `[!…]`: a negated glob bracket --------------------------------------
# Nothing matches these patterns, so they stay literal — the point is that they
# reach the globber at all instead of being eaten as events.
echo x[!a]y
echo "q[!a]r"
echo ${arr[!1]-n}
# …but a `[` with no `]` on the line does not inhibit.
echo before-2
echo [!q
echo "rc=$?"
# Nor does a `!` that is not the first thing in the bracket.
echo a[b!c]
echo "rc=$?"

# The bracket rule has one exception, and it is the expansion character itself:
# bash compares the character *after* the `!` against `history_expansion_char`,
# so a doubled `!` gets through where every other designator is turned away.
# Each expanding line is preceded by a fixed line so that what `!!` names is
# predictable — and bash echoes the line it expanded, so these print twice.
echo one two
echo [!!]
echo one two
echo [!!:1]
echo one two
echo x[!!]y
echo one two
echo [[!!]]
# The `!` of a `[!]…]` bracket — where the `]` is a literal member rather than
# the close — is inhibited like any other, and that does not carry over to a
# later `!!` on the same line.
echo one two
echo [!]!!]
# `[!!!]` is therefore the doubled `!` followed by a leftover `!]`, which is an
# event reference like any other and fails.
echo before-2b
echo [!!!]
echo "rc=$?"
# Every other designator directly after the `[` stays inhibited.
echo [!^]
echo [!$]
echo [!:0]
echo [!-1]
echo [!*]
echo [!%]
echo a[!$]b

# --- `!(pat)`: extglob negation ------------------------------------------
# With extglob OFF, `(` is *not* one of bash's history_no_expand_chars, so this
# is an event reference and fails. This is the rule that looks most like a bash
# bug and is most tempting to "fix" in a reimplementation.
shopt -u extglob
echo before-3
echo !(zzz)
echo "rc=$?"
# With it on, `!(` is a pattern — including the empty `!()` form.
shopt -s extglob
case abc in !(xyz)) echo negated-matched ;; *) echo no ;; esac
case abc in x!(yz)) echo no ;; *) echo second-not-matched ;; esac
# …but only when the group is closed, and only when the `!` is at least two
# characters into the line (bash's own `i > 1`), so these two still fail.
echo before-3b
echo !(zzz
echo "rc=$?"
x!(y)
echo "rc=$?"

# --- an assignment inhibits nothing --------------------------------------
echo before-4
v=!q
echo "rc=$? v=[${v-unset}]"

# --- quoting --------------------------------------------------------------
# Single quotes suppress expansion; double quotes do not, because the rewrite
# happens before quote removal.
echo '!q'
echo before-5
echo "!q"
echo "rc=$?"
# A backslash suppresses it and is left for the parser to remove.
echo \!q
# Inside a double-quoted string the closing quote joins the no-expand set — but
# only immediately after the `!`; one character further on it merely ends the
# event's search string, so `!a` is a real reference that fails.
echo "x !"
echo "!"
echo before-5b
echo "x !a"b
echo "rc=$?"

# --- where an event's search string ends ----------------------------------
# The search string runs to the first `history_word_delimiters` character or the
# first word-designator/modifier introducer, so the failure messages differ in
# how much of the line they quote back. `=`, `{`, `[`, `!` and `'` are *not*
# delimiters; `;`, `|`, `&`, `<`, `>`, `(`, `)`, `-` and `%` are.
# No event starts with `zz`, so every one of these fails and the message shows
# exactly how much of the line bash took to be the search string.
echo before-6
echo !zz(q)
echo !zz=b
echo !zz[b]
echo !zz!b
echo !zz'b'
echo !zz-b
echo !zz%b
echo !zz;b
echo !zz|b
echo !zz<b
echo "rc=$?"
# An empty search string before a delimiter matches nothing and is reported as a
# bare `!`; before a designator it means the previous event, which is what makes
# `!:0` and `!$` work at all.
echo before-7
echo !;x
echo "rc=$?"

# --- `!` that cannot start a designator at all ---------------------------
echo ! ok
echo trailing-bang !
echo "$((1!=2))"
[[ ! -e /nonexistent-zzz ]] && echo negation-ok
echo done

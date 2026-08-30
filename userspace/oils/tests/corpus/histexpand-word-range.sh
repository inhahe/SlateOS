# Word designators: which words a designator names, and which designators bash
# rejects outright rather than clamping. A designator that names a word the event
# does not have is a *failure* — `history -p` prints `history expansion failed`
# and returns 1 — so this case checks the pass/fail split as much as the text.
#
# `history -s` records without running and `history -p` expands without running
# the result, so nothing here has any other effect. The event is `a b c d`:
# word 0 is the command `a` and words 1-3 are its arguments.
#
# Every probe re-records the event on the line just before it, because a script
# line is itself added to the history as it is read: without the fresh `history
# -s`, a `!!` in the *second* of two consecutive `history -p` lines would name
# the first one rather than the intended event.
set -o history
set -H

# --- the plain shapes ---------------------------------------------------------
history -s 'a b c d'
history -p '!!:0' '!!:1' '!!:3' '!!:^' '!!:$' '!!:*'
# The `:` may be left off for `^`, `$`, `*` and `-`…
history -s 'a b c d'
history -p '!^' '!$' '!*' '!!^' '!!$' '!!*' '!!-'
# …but not for a number: `!!1` is the whole event with a literal `1` after it.
history -s 'a b c d'
history -p '!!1' '!!2'

# --- ranges -------------------------------------------------------------------
history -s 'a b c d'
history -p '!!:1-2' '!!:0-0' '!!:3-3' '!!:0-$' '!!:^-$' '!!:^-^' '!!:1-^'
# A range with no end runs to the *second-to-last* word, not the last.
history -s 'a b c d'
history -p '!!:0-' '!!:2-' '!!:^-' '!!:-' '!!:-0' '!!:-2' '!!:-$' '!!:-^'
history -s 'a b c d'
history -p '!!-2' '!!-$' '!!^-2'
# That defaulted end may fall before the start, which is empty rather than an
# error — unlike a written-out end that does the same (see `!!:3-1` below).
history -s 'a b c d'
history -p '!!:3-'; echo "open-backwards=$?"
# `n*` runs to the last word.
history -s 'a b c d'
history -p '!!:0*' '!!:3*' '!!:^*'

# --- a bare `^` can stand in for `-^` as the end of a range -------------------
# So `!!:0^` is words 0 through 1. Only one may: the second `^` of `!!:^^^` is
# literal text, and a `$` or a number never stands in this way.
history -s 'a b c d'
history -p '!!:0^' '!!:1^' '!!:^^' '!!:^^^' '!!:^2' '!!:2$' '!!:^$'
history -s 'a b c d'
history -p '!!:1-^^' '!!:-^^'

# --- `$` and `*` end the designator, so what follows them is literal ----------
# This is the one place `^` and a number differ from them: `!!:^-` and `!!:2-`
# read a range, while `!!:$-` is the last word and a literal `-`.
history -s 'a b c d'
history -p '!!:$-' '!!:$$' '!!:$^' '!!:$*' '!!:$z' '!!$-'
history -s 'a b c d'
history -p '!!:*-' '!!:*$' '!!:*^' '!!:*x' '!!*-'
history -s 'a b c d'
history -p '!!:--' '!!:1-*' '!!:^z' '!!:^-z' '!!:2*x'

# --- out of range is an error, not a clamp -----------------------------------
# A start past the last word…
history -s 'a b c d'
history -p '!!:4' 2>/dev/null; echo "start-past-end=$?"
history -s 'a b c d'
history -p '!!:9' 2>/dev/null; echo "start-way-past=$?"
history -s 'a b c d'
history -p '!!:4-' 2>/dev/null; echo "start-past-end-open=$?"
history -s 'a b c d'
history -p '!!:4*' 2>/dev/null; echo "start-past-end-star=$?"
# …an end past the last word…
history -s 'a b c d'
history -p '!!:1-9' 2>/dev/null; echo "end-past-end=$?"
history -s 'a b c d'
history -p '!!:-9' 2>/dev/null; echo "dash-end-past-end=$?"
# …and a written-out end before the start.
history -s 'a b c d'
history -p '!!:3-1' 2>/dev/null; echo "backwards=$?"
history -s 'a b c d'
history -p '!!:2^' 2>/dev/null; echo "backwards-caret=$?"
# A trailing modifier does not save the designator.
history -s 'a b c d'
history -p '!!:4:h' 2>/dev/null; echo "with-modifier=$?"
# The message itself, so its full text is diffed at least once.
history -s 'a b c d'
history -p '!!:4'; echo "message-rc=$?"

# --- the same edges on a one-word event, where word 1 does not exist ----------
history -s 'onlyone'
history -p '!!:0' '!!:$' '!$'
# `*` names words 1 through the last and is simply *empty* here, not an error —
# and so is a range whose end was defaulted.
history -s 'onlyone'
history -p '!!:*' '!*' '!!:-' '!!:-0' '!!:0-0' '!!:0*'
history -s 'onlyone'
history -p '!!:$-' '!!:*-' '!!:--'
# Everything that actually reaches for word 1 fails.
history -s 'onlyone'
history -p '!!:^' 2>/dev/null; echo "caret=$?"
history -s 'onlyone'
history -p '!^' 2>/dev/null; echo "bare-caret=$?"
history -s 'onlyone'
history -p '!!:2-' 2>/dev/null; echo "two-open=$?"
history -s 'onlyone'
history -p '!!:3*' 2>/dev/null; echo "three-star=$?"
history -s 'onlyone'
history -p '!!:-2' 2>/dev/null; echo "dash-two=$?"
history -s 'onlyone'
history -p '!!:1-2' 2>/dev/null; echo "one-two=$?"
history -s 'onlyone'
history -p '!!:^-$' 2>/dev/null; echo "caret-dollar=$?"
history -s 'onlyone'
history -p '!!:^-^' 2>/dev/null; echo "caret-caret=$?"
echo done

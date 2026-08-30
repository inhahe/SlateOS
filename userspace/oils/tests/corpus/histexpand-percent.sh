# The `%` word designator: the word that the most recent `?string?` event search
# matched. It is remembered for the life of the shell, so it names a word of
# whatever event that search found — not of the event the designator is applied
# to, which is why `!%` works on a line whose own event has no such word.
#
# `history -s` records without running and `history -p` expands without running
# the result, so nothing here has any other effect. Two harness properties are
# doing quiet work in the layout below, both of them bash's:
#
#   * a script line is added to the history as it is read, so a probe would
#     otherwise see the *previous line* as `!!`;
#   * `history -s` deletes the last entry before adding its argument, which is
#     that just-read line — so a `history -s` on the line before each probe both
#     records the event and clears the probe before it out of the way. That
#     matters here beyond the numbering: a leftover probe line contains the
#     search string it was searching for, and would be found in preference to
#     the intended event.
set -o history
set -H

# --- with no search yet, `%` is empty, and empty is not a failure -------------
history -s 'alpha beta gamma'
history -p '!%'; echo "unset-bare=$?"
history -s 'alpha beta gamma'
history -p '!!:%' '!!%'; echo "unset-colon=$?"
# An empty search string, though, is not "match anything": it means the previous
# search string, and with no previous search there is nothing to reuse. These
# two probes have to come before any successful search below, because the
# memory they depend on being empty outlives the line that fills it.
history -s 'alpha beta gamma'
history -p '!??' 2>/dev/null; echo "empty-search=$?"
history -s 'alpha beta gamma'
history -p '!?' 2>/dev/null; echo "unterminated-empty-search=$?"

# --- a search records the word it matched ------------------------------------
history -s 'alpha beta gamma'
history -p '!?gam?' '!%' '!!:%'
# …and keeps it afterwards, for events that have no such word at all: this is a
# remembered string, not an index into the event in hand.
history -s 'one two'
history -p '!%' '!!:%' '!!:1'

# --- which word: the one holding the LAST occurrence of the search string -----
# The line is scanned backwards, so `?a?` here is the `a` of `za`, not of `xa`.
history -s 'xa ya za'
history -p '!?a?' '!%'
history -s 'alpha beta gamma'
history -p '!?ha bet?' '!%'
# An operator is a word of its own, so a match that starts on one is that word.
history -s 'alpha; beta gamma'
history -p '!?; bet?' '!%'
history -s 'alpha (beta) gamma'
history -p '!?(bet?' '!%'
# Whitespace, though, belongs to no word — a match starting there is remembered
# as nothing, and `%` is empty again.
history -s 'alpha beta gamma'
history -p '!? bet?' '!%'; echo "match-on-space=$?"

# --- `%` ends the designator, and is no kind of endpoint ----------------------
history -s 'alpha beta gamma'
history -p '!?gam?' '!!:%*' '!!:%-' '!!:%%' '!!:%$' '!!:%1' '!!:%x'
# So a `%` where a range endpoint would go is literal text, and the range gets
# the end it would have had with nothing there at all.
history -s 'alpha beta gamma'
history -p '!?gam?' '!!:1%' '!!:1-%' '!!:-%' '!!:*%' '!!:$%'
# Modifiers apply to it like any other selection.
history -s 'alpha beta gamma'
history -p '!?gam?' '!!:%:h' '!!:%:q' '!%%'
# It never fails, even on a one-word event, because it does not look at one.
history -s 'onlyone'
history -p '!?gam?' >/dev/null
history -s 'onlyone'
history -p '!%' '!!:%'; echo "one-word-event=$?"

# --- which searches record it, and which do not -------------------------------
# A `!string` prefix search does not…
history -s 'alpha beta gamma'
history -p '!?gam?' >/dev/null
history -s 'quux corge'
history -p '!quu' '!%'
# …nor does a `^old^new^` quick substitution…
history -s 'one two three'
history -p '^two^TWO^' '!%'
# …and a search that finds nothing leaves the previous match alone.
history -s 'alpha beta gamma'
history -p '!?nosuchthing?' 2>/dev/null; echo "failed-search=$?"
history -s 'zed'
history -p '!%'
# A search that succeeds and only *then* fails still records: the word is taken
# when the event is found, and the failure does not roll it back.
history -s 'delta epsilon'
history -p '!?eps?:9' 2>/dev/null; echo "search-then-bad-word=$?"
history -s 'zed'
history -p '!%'

# --- an empty search string means the previous search string ------------------
# `!??` after a `!?gam?` searches for `gam` again, so it finds the older event
# rather than the most recent one.
history -s 'alpha beta gamma'
history -p '!?gam?' >/dev/null
history -s 'nothing here'
history -p '!??' '!%'
# A search that finds nothing fails whether or not it was terminated.
history -s 'nothing here'
history -p '!?zzz?' 2>/dev/null; echo "no-match-rc=$?"
history -s 'nothing here'
history -p '!?zzz?'; echo "no-match-message-rc=$?"
history -s 'nothing here'
history -p '!?zzz'; echo "unterminated-message-rc=$?"
echo done

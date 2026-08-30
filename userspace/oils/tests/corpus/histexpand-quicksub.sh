# `^old^new^` looks like a form of its own, but readline implements it by simply
# prefixing `!!:s` to the line and running the ordinary expander over the result.
# Everything surprising about it follows from that: it works only at the very
# start of a line, its diagnostics quote back the rewritten `:s^…^…^` spec rather
# than what was typed, an empty history fails as `!!` rather than as the
# substitution, and — the part most likely to be got wrong — whatever follows the
# closing delimiter *survives and is itself expanded*.
#
# Each use re-issues its own setup line first, so the event being rewritten is
# always the line directly above.
set -o history
set -H

# --- the degenerate forms, before any substitution has been recorded --------
# An empty `old` reuses the previous substitution, and there is none yet.
echo before-1
^
echo "rc=$?"
^^
echo "rc=$?"
^^^
echo "rc=$?"

# --- an explicit pattern that the previous line does not contain -----------
echo before-2
^zz^two^
echo "rc=$?"

# --- the ordinary forms ----------------------------------------------------
echo one two
^one^X^
# The trailing delimiter may be left off.
echo one two
^one^X
# An empty replacement deletes.
echo one two
^one^

# --- everything after the closing delimiter survives -----------------------
echo one two
^one^X^^
echo one two
^one^X^y
echo one two
^one^X^ tail
# …including a redirection or a pipeline, since the rewritten line is re-parsed.
echo one two
^one^X^ | cat
# …and including another event reference, which is expanded in its turn.
echo one two
^one^X^ !!

# --- and the modifier scan is reached the same way -------------------------
# `:p` previews without running, so `$?` is left alone.
echo one two
^one^X^:p
echo "rc=$?"
# An unknown modifier abandons the line, exactly as after a `!!`.
echo one two
^one^X^:2
echo "rc=$?"

# --- where it does *not* apply ---------------------------------------------
# One leading space is enough to make it an ordinary (nonexistent) command.
echo one two
 ^one^X^
echo "rc=$?"
# Quoting the whole thing keeps it literal — the `^` is no longer first.
echo one two
echo '^one^X^'
echo "^one^X^"

# --- an empty history fails on the `!!`, not on the substitution -----------
history -c
^a^b^
echo "rc=$?"
^a^b^ tail
echo "rc=$?"
echo done

# bash expands history one *physical* line at a time, and it tells readline which
# quote its reader is currently inside (readline's `history_quoting_state`, set
# from bash's delimiter stack). So a `!` on the second line of a multi-line
# command is judged in the context of the first line's still-open quote: inert
# inside `'…'`, live inside `"…"` but with the `#`-comment rule switched off.
#
# Each case re-issues `echo one` first, so `!!` always names the line above it.
set -o history
set -H

# --- a single quote carried across the newline makes the whole line inert -----
echo one
echo 'x
!!'
echo one
echo 'x
!! z'
# …including an event that does not exist, which is therefore not an error.
echo one
echo 'x
!zz'
# …and including the `^old^new^` rewrite, which readline still performs — the
# `!!` it prefixes is simply quoted, so the literal spec is what runs.
echo one
echo 'x
^one^two^'

# --- but only up to the quote's close ----------------------------------------
echo one
echo 'x
y' !!

# --- a carried double quote joins the no-expand set for a lone `!` ------------
echo one
echo "x
!"

# --- a carried double quote leaves expansion live ------------------------------
# Inside `"` a `#` does not start a comment, so the `!!` after one is reached.
echo one
echo "a
#!!"
# But readline's *in-line* scan toggles on a `'` even inside a double-quoted
# string — a separate rule from the carried state, and the reason this `!!`,
# which the `'` on the line above opened a single-quoted span over, is literal.
echo one
echo "a 'b
c' !!"

# --- `$( … )` pushes its inner quote; a backquote body does not ---------------
# bash re-parses a `$( … )` body, so a quote opened inside it becomes the
# reader's state…
echo one
echo $(echo 'x
!!')
# …while a backquote body is scanned as one flat matched pair, so the quote
# inside it never becomes the state and the `!!` expands.
echo one
echo `echo 'y
!!'`

# --- a `\` continuation still offers the joined line for expansion ------------
# The lexer *deletes* the `\<newline>`, so the text so far parses as the complete
# command `echo x` — but the reader is still waiting, and the `!!` expands.
echo one
echo x \
!!
# A `\` inside a single-quoted string is not a continuation, so nothing is
# carried and the closing line is an ordinary one.
echo one
echo 'x \
!!'

# --- a compound command's later lines are outside any quote ------------------
echo one
if true; then
echo !!
fi

# --- a failed expansion on a continuation line drops just that line ----------
# The command stays unterminated, so the reader keeps going and the next line
# closes it — exactly as bash does.
echo one
echo "x
!zz"
closed"
echo "rc=$?"
echo one
echo "a
#!zz"
closed"
echo "rc=$?"
echo done

# `$( … )` is the one construct whose unterminated-`)` is NOT reported on the
# line it opened on: bash scans the substitution body without parsing it, then
# re-parses it once the outer word is complete, so the failure is discovered at
# *end of input* and is reported one line past the last source line (bash's
# usual EOF line quirk). Process substitution `<( … )` / `>( … )` behaves the
# same way. See lex-error-line-quote.sh for the opening-line rule that every
# other construct follows.
#
# Verified against bash 5.2.37.

echo one
echo two
v=$(echo a
echo b
echo c

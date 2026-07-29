# `fc` — the history editor, and the one builtin that both reads *and* rewrites
# the list `history` prints. Three modes hide behind one name:
#
#   * `-l` lists a range, `%d\t %s` — a tab and a space, not `history`'s two
#     leading spaces and %5d field;
#   * `-s` (a.k.a. `-e -`) re-runs one command after a *global* `pat=rep`
#     substitution, echoing what it settled on to stderr first;
#   * bare `fc` dumps the range into a temporary file, runs `$FCEDIT`, then
#     `$EDITOR`, then `vi` on it, and re-reads whatever comes back — with the
#     reader's echo (`set -v`) turned on for that re-read only.
#
# The range arithmetic is the fiddly part and most of this case is about it: a
# bare number is an absolute history number, a negative one counts back from
# the newest, `0` names the newest usable entry, and anything else is a prefix
# searched newest-first. The two editing modes un-record their own line before
# resolving any of that, which is why `fc` names the command *before* it while
# `fc -l` names itself.

set -o history

echo "=== -l lists a number, a tab and a space"
echo one
echo two
echo three
fc -l | cat -A | sed -n '1,2p'

echo "=== the whole list so far"
fc -l

echo "=== -n drops the number but keeps the separator"
fc -l -n | cat -A | sed -n '1,2p'

echo "=== -r reverses"
fc -l -r 2 4

echo "=== an explicit range by number"
fc -l 2 4

echo "=== first > last reverses on its own"
fc -l 4 2

echo "=== a negative number counts back from the newest"
fc -l -3

echo "=== 0 in a listing is the newest entry, which is the fc line itself"
fc -l 0

echo "=== one endpoint lists just that entry"
fc -l 3

echo "=== a string endpoint is a prefix, searched newest first"
fc -l 'echo two'

echo "=== a prefix that matches nothing"
fc -l nosuchcommand; echo "  rc=$?"

echo "=== an out-of-range number clamps to the end it came from"
fc -l 900 901

echo "=== -0 outside a listing is out of range"
fc -0; echo "  rc=$?"

echo "=== an unknown option, and -e with nothing after it"
fc -Z; echo "  rc=$?"
fc -e; echo "  rc=$?"

echo "=== -s echoes the command to stderr, then runs it"
echo aaa bbb aaa
fc -s aaa=XXX

echo "=== the replacement is global and splits at the first ="
echo k1
fc -s k1=k2=k3

echo "=== -s with no operand just re-runs the previous command"
echo repeat-me
fc -s

echo "=== -s names a command by prefix too, and the prefix starts the line"
echo pick-this-one
echo and-not-this
fc -s pick; echo "  rc=$?"
fc -s 'echo pick'

echo "=== -s replaced its own entries with what it ran"
fc -l -8

echo "=== the editor gets a file holding the range; an unchanged file re-runs"
FCEDIT=cat
echo edit-me
fc

echo "=== a rewriting editor decides what runs"
rewrite() { echo 'echo rewritten-by-editor' > "$1"; }
FCEDIT=rewrite
echo will-be-replaced
fc

echo "=== an editor that fails runs nothing and returns 1"
FCEDIT=false
echo not-reached
fc; echo "  rc=$?"

echo "=== an editor that empties the file runs nothing"
blank() { : > "$1"; }
FCEDIT=blank
echo also-not-reached
fc; echo "  rc=$?"

echo "=== FCEDIT beats EDITOR"
fc_ed() { echo "  FCEDIT ran"; }
ed_ed() { echo "  EDITOR ran"; }
FCEDIT=fc_ed
EDITOR=ed_ed
echo chosen-by-fcedit
fc

echo "=== with FCEDIT unset, EDITOR is used"
unset FCEDIT
echo chosen-by-editor
fc

echo "=== -e names an editor for one call"
echo chosen-by-dash-e
fc -e fc_ed

echo "=== -e - is -s"
echo dash-e-dash
fc -e - 'echo dash-e'

echo "=== a range hands the editor every command in it, re-read in order"
FCEDIT=cat
echo first-of-range
echo second-of-range
fc -3 -2

echo "=== and -r hands them over backwards"
echo one-of-two
echo two-of-two
fc -r -3 -2

echo "=== fc -l keeps its own line; the editing forms drop theirs"
fc -l -4

# `select` measures each item with bash's `displen` (`execute_cmd.c`), which is
# `wcswidth` of the decoded item — *terminal columns*, not characters and not
# bytes. The width feeds the column stride, so getting it wrong changes the
# shape of the whole menu and not just one gutter.
#
#   displen(s):
#     slen = mbstowcs (wcstr, s, 0);
#     if (slen == -1) slen = 0;
#     …
#     wclen = wcswidth (wcstr, slen);
#     return (wclen < 0 ? STRLEN(s) : wclen);
#
# The two fallbacks in there are *different*, which is the subtle part:
#
#   * a string that will not decode takes `slen = 0`, so `wcswidth` is called
#     over no characters and returns 0 — the item measures **nothing**, however
#     many bytes it has;
#   * a string that decodes but holds a character with no printable width makes
#     `wcswidth` return -1, and only then does the item measure its **byte
#     length**.
#
# Only assigned code points are used below. The host's `wcwidth` and the Unicode
# database disagree about *unassigned* ones inside East Asian blocks (see
# known-issues TD-OILS-UNASSIGNED-CODE-POINTS-TAKE-THE-UNICODE-WIDTH-NOT-THE-HOSTS),
# and pinning that disagreement here would pin the host's libc version.
#
# Tabs are spelled out, as in `select-menu.sh`; the menu goes to stderr.
show() { sed 's/\t/<TAB>/g'; }
menu() { COLUMNS=$1 "$BASH" -c "select o in $2; do break; done" </dev/null 2>&1 | show; }

echo "=== a wide item is two columns per character, not one"
menu 80 '日本語 ab cd ef gh ij kl mn'
echo "=== …and the same list with an ASCII item of the same character count"
menu 80 'abc ab cd ef gh ij kl mn'

echo "=== a combining mark takes no column of its own"
menu 40 "$(printf 'a\xcc\x80') bb cc dd ee ff"
echo "=== …which is not what counting characters would give"
menu 40 'ab bb cc dd ee ff'

echo "=== Hangul: the leading consonant is wide, the vowel and final are not"
menu 40 '각 ab cd ef gh ij'
echo "=== fullwidth forms are wide too"
menu 40 'ＡＢ cd ef gh ij kl'

# The two fallback probes spell their item as an ANSI-C literal for the *inner*
# shell to decode, rather than substituting the bytes here. A byte that is no
# character cannot survive this host's child-process argv (see known-issues
# TD-OILS-A-NON-UTF-8-ARGUMENT-IS-REPLACED-ON-THE-WINDOWS-HOST), so passing one
# through `"$BASH" -c` would measure that limitation instead of `displen`.
echo "=== a control character makes the item measure its bytes"
menu 40 "\$'\x01日本' ab cd ef gh"
echo "=== …7 bytes, so the same stride as a 7-column ASCII item"
menu 40 'xxxxxxx ab cd ef gh'

echo "=== an item that will not decode measures nothing at all"
menu 40 "\$'\xff\xff\xff\xff\xff\xff' ab cd ef gh ij"
echo "=== …the same stride as an empty item"
menu 40 '"" ab cd ef gh ij'

echo "=== the widest item is the one the stride comes from"
menu 60 'a 日本語日本語 b c'

echo "still here"

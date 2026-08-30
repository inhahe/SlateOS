# An option letter, to `getopts`, is one *byte*. The scan takes the next byte
# out of the argument and looks for that byte in the optstring; it does not
# decode the text. So a multi-byte character in the arguments is as many options
# as it has bytes, and one in the optstring is as many separate one-byte options
# as it has bytes — which is why `-é` matches an optstring of `é` twice, once
# per byte, and why an optstring of `é` accepts either of `-\xc3` and `-\xa9`.
#
# A colon is the one byte that is never an option, in either place. In the
# argument it is refused however the optstring is written, because the colon in
# an optstring is the marker that the letter *before* it takes an argument and
# is never a letter itself. In the optstring a leading colon is the marker that
# selects silent mode, and it is stripped before the search — so an optstring of
# `::a:` is really `:a:` with silent mode on, whose first letter is another
# colon that nothing can match.
#
# That stripped-once reading is visible where a missing option argument is
# reported, because the answer is arrived at twice over. The scanner picks the
# character to return by looking at the *first* byte of the optstring it was
# handed, and separately leaves an empty `OPTARG` behind as the sign of what
# went wrong; the builtin then reads that sign back to tell a missing argument
# from an unknown option, and rewrites `OPTARG` to the offending letter.
#
# With one leading colon the scanner sees `a:` and returns `?`, so the sign is
# read, and `OPTARG` ends up holding `a`. With two the scanner sees `:a:`,
# returns the `:` itself, and the builtin — which was only ever looking for a
# `?` — passes it straight through with the empty `OPTARG` still in place. Both
# store `:` in the name; only the first says which letter it was about.

echo "=== a multi-byte character in the arguments is one option per byte"
OPTIND=1; set -- -é
getopts "ab" o; echo "  rc=$? o=$o ind=$OPTIND"
getopts "ab" o; echo "  rc=$? o=$o ind=$OPTIND"

echo "=== and one in the optstring matches byte by byte"
OPTIND=1; set -- -é
while getopts "é" o; do echo "  o=[$o] ind=$OPTIND"; done
echo "  end rc=$? ind=$OPTIND"

echo "=== so the bytes of it are separate options"
OPTIND=1; set -- $'-\xc3'
getopts "é" o; echo "  rc=$? o=[$o] ind=$OPTIND"
OPTIND=1; set -- $'-\xa9'
getopts "é" o; echo "  rc=$? o=[$o] ind=$OPTIND"

echo "=== a byte of one is still a byte after a letter that is not"
OPTIND=1; set -- -aé
getopts "a" o; echo "  rc=$? o=[$o] ind=$OPTIND"
getopts "a" o; echo "  rc=$? o=[$o] ind=$OPTIND"

echo "=== a colon in the arguments is never an option"
OPTIND=1; set -- -:
getopts ":ab" o; echo "  rc=$? o=[$o] arg=[${OPTARG-unset}] ind=$OPTIND"
OPTIND=1; set -- -:
getopts "a:b" o; echo "  rc=$? o=[$o] arg=[${OPTARG-unset}] ind=$OPTIND"
OPTIND=1; set -- -:
getopts "::a:" o; echo "  rc=$? o=[$o] arg=[${OPTARG-unset}] ind=$OPTIND"

echo "=== one leading colon is stripped, and it selects silent mode"
OPTIND=1; set -- -a
getopts "a:" v; echo "  [a:]   rc=$? v=[$v] arg=[${OPTARG-unset}] ind=$OPTIND"
OPTIND=1; set -- -a
getopts ":a:" v; echo "  [:a:]  rc=$? v=[$v] arg=[${OPTARG-unset}] ind=$OPTIND"
OPTIND=1; set -- -a
getopts "::a:" v; echo "  [::a:] rc=$? v=[$v] arg=[${OPTARG-unset}] ind=$OPTIND"
OPTIND=1; set -- -a
getopts ":::a:" v; echo "  [:::a:] rc=$? v=[$v] arg=[${OPTARG-unset}] ind=$OPTIND"

echo "=== with the argument supplied there is nothing to choose between"
OPTIND=1; set -- -a x
getopts "::a:" v; echo "  rc=$? v=[$v] arg=[${OPTARG-unset}] ind=$OPTIND"

echo "=== and an unknown option is silent-mode's own answer"
OPTIND=1; set -- -z
getopts "::a:" v; echo "  rc=$? v=[$v] arg=[${OPTARG-unset}] ind=$OPTIND"

echo "=== a letter with no colon after it takes no argument"
OPTIND=1; set -- -a
getopts "::a" v; echo "  rc=$? v=[$v] arg=[${OPTARG-unset}] ind=$OPTIND"

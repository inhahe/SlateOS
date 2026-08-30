# Pathname expansion: ordering, no-match behaviour, character classes, dotfiles,
# and the shopt switches that change each of those.
touch a.txt b.txt c.log .hidden
mkdir -p sub && touch sub/d.txt

# Matches sort in the collation order, and a glob that matches nothing is left
# alone as a literal word (the default, `nullglob` off).
echo *.txt
echo *.md
echo "count=$(set -- *.txt; echo $#)"
echo "nomatch-count=$(set -- *.md; echo $#)"

# `?` matches exactly one character; `[...]` a set; `[!...]` a complement.
echo ?.txt
echo [ab].txt
echo [!a].txt

# A leading dot is never matched by a leading `*` — dotfiles need an explicit
# dot (or `dotglob`).
echo .*
shopt -s dotglob
echo "dotglob: $(echo *)"
shopt -u dotglob

# nullglob: a non-matching pattern expands to nothing at all, so the word
# disappears rather than surviving literally.
shopt -s nullglob
echo "nullglob-count=$(set -- *.md; echo $#)"
shopt -u nullglob

# Globs apply per path component, and `*` does not cross `/`.
echo */*.txt
echo *.txt sub/*.txt

# Quoting disables globbing entirely.
echo "*.txt" '*.txt'

# A glob in a case pattern and in an assignment RHS is NOT pathname-expanded.
v=*.txt
echo "assigned=[$v]"
echo "expanded=[$(echo $v)]"

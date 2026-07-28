# The `shopt` options that change what a glob matches, and `extglob` patterns.
# Every case runs in a directory the case itself creates, so the listing is
# deterministic.

mkdir -p gdir && cd gdir || exit 1
: > a.txt; : > b.txt; : > c.log; : > .hidden; : > .hidden.txt
mkdir -p sub; : > sub/nested.txt

# By default a glob that matches nothing is left literal, and dotfiles are
# excluded even from `*`.
echo *.txt
echo *
echo nomatch-*

# nullglob makes a non-matching pattern vanish instead.
shopt -s nullglob
echo "nullglob:[$(echo nomatch-*)]"
count() { echo "argc=$#"; }
count nomatch-*
shopt -u nullglob
count nomatch-*

# failglob makes it an error; the command does not run.
shopt -s failglob
echo failglob-should-not-print nomatch-* 2>/dev/null
echo "failglob-status=$?"
shopt -u failglob

# dotglob includes names beginning with a dot — but never `.` or `..`.
shopt -s dotglob
echo *.txt
echo *
shopt -u dotglob

# nocaseglob matches case-insensitively.
: > UPPER.TXT
echo upper.txt
shopt -s nocaseglob
echo upper.txt
shopt -u nocaseglob
rm -f UPPER.TXT

# A glob never crosses a `/`, so `*` alone does not reach into `sub`.
echo *.txt sub/*.txt

# Character classes and ranges.
echo [ab].txt
echo [!a]*.txt
echo [^a]*.txt
echo ?.txt
echo [[:alpha:]].txt

# extglob adds the ?() *() +() @() !() operators.
shopt -s extglob
echo @(a|b).txt
echo +([ab]).txt
echo !(a).txt
echo ?(a|b|c).txt
echo *(a|b).txt
# extglob patterns also work in `case` and in `${var##pat}`.
f=a.txt
case $f in
  @(a|b).txt) echo "case-extglob=matched" ;;
  *) echo "case-extglob=no" ;;
esac
path=/one/two/three
echo "${path##*(/)}"
echo "${path//@(one|three)/X}"
shopt -u extglob

# `[[ str == pat ]]` globs the right-hand side but never expands filenames.
[[ a.txt == *.txt ]] && echo "dbl-bracket-glob=yes"
[[ a.txt == "*.txt" ]] || echo "dbl-bracket-quoted=no"

# globstar makes `**` cross directory boundaries.
shopt -s globstar
echo **/*.txt
shopt -u globstar
echo **/*.txt

cd .. || exit 1
rm -rf gdir

# A `$PATH` hit is not only spawned, it is *reported* — by `command -v`, by
# `type`, by `type -P`, by `hash`, and by `$BASH_SOURCE` when `.` did the
# searching. What all of those print is the `$PATH` entry joined to the name,
# with neither half tidied on the way:
#
#   * a relative entry stays relative, so `PATH=bin` answers `bin/tool` and not
#     the absolute path that entry happens to name today — and it goes on
#     meaning "relative to the working directory", so the same entry answers
#     differently after a `cd`;
#   * a leading `./` is kept, because the entry said it;
#   * a trailing `/` is not doubled and not dropped: `PATH=bin//` answers
#     `bin//tool`.
#
# An *empty entry* is the one that is not reported as written — it names the
# current directory and is spelled `.`. An empty `$PATH` as a whole is a
# different thing again: it searches the working directory by name.
#
# A word that already spelled a path was never a search, and comes back as
# typed.

mkdir -p bin sub
printf '#!/bin/sh\necho tool\n' > bin/tool
printf '#!/bin/sh\necho here\n' > here
printf 'echo "  src=$BASH_SOURCE"\n' > bin/lib.sh
chmod +x bin/tool here
SAVE=$PATH

echo "=== a relative entry stays relative"
PATH=bin
echo "  cv=[$(command -v tool)]"
echo "  type=[$(type tool)]"
echo "  typeP=[$(type -P tool)]"
echo "  typea=[$(type -a tool)]"

echo "=== and it is what the hash table remembers"
hash -r
type -P tool > /dev/null
hash
hash -r

echo "=== a leading ./ is kept"
PATH=./bin
echo "  cv=[$(command -v tool)]"

echo "=== a trailing slash is neither doubled nor dropped"
PATH=bin//
echo "  cv=[$(command -v tool)]"
PATH=bin/
echo "  cv=[$(command -v tool)]"

echo "=== an absolute entry answers absolutely"
PATH=$PWD/bin
x=$(command -v tool)
echo "  cv=[${x#"$PWD"}]"

echo "=== the entry means the working directory, so a cd moves it"
PATH=bin
echo "  before=[$(command -v tool)]"
cd sub
PATH=../bin
echo "  after=[$(command -v tool)]"
PATH=bin
echo "  gone st=$(command -v tool > /dev/null; echo $?)"
cd ..

echo "=== an empty entry is the current directory, spelled ."
PATH=":bin"
echo "  cv=[$(command -v here)]"
PATH="bin:"
echo "  cv=[$(command -v here)]"

echo "=== an empty PATH searches the working directory by name"
PATH=
y=$(command -v here)
echo "  cv=[${y#"$PWD"}]"

echo "=== a word that spelled a path comes back as typed"
PATH=$SAVE
echo "  cv=[$(command -v ./bin/tool)]"
echo "  cv=[$(command -v bin/tool)]"
echo "  cv=[$(command -v ./bin/../bin/tool)]"

echo "=== and . reports the file it actually read"
PATH=bin
. lib.sh
cd sub
PATH=../bin
. lib.sh
cd ..
PATH=$SAVE

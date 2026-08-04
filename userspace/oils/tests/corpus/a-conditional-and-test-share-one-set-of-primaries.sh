# `[[ … ]]` and the `test`/`[` builtin do not each have their own idea of what
# a unary primary is: bash draws both from one table, so every one of the 26
# primaries parses in both spellings and answers the same for the same operand.
# The pairs below are the whole of it — file kinds (`-b -c -p -S`), mode bits
# (`-u -g -k`), ownership (`-O -G`), freshness (`-N`), the older `-a` spelling
# of `-e`, and the three that ask about shell state rather than the filesystem
# (`-v` set, `-o` enabled, `-R` a nameref) — and a primary outside the table is
# a *syntax* error in `[[ ]]` while `[` reports it as an operand.
#
# `-a` is the sharpest case, because the same two letters are also `[`'s `and`
# connective. Position decides and nothing else: leading, it is the primary, so
# `[[ -a f ]]` asks whether `f` exists; between two words it is a connective `[`
# honours and `[[ ]]` has no grammar for at all.
#
# `-R` asks about the *attribute*, not the target, which is why a nameref
# pointing at nothing still tests true while `-v` on the same name tests false.
# And a synonym keeps the spelling it was written with: `-h` never comes back
# out of `declare -f` as its twin `-L`.

t() { if eval "$1"; then echo "  yes  $1"; else echo "  no($?) $1"; fi; }

touch f; mkdir -p d; echo x > g

echo "=== every primary parses inside [[ ]], and agrees with [ ]"
for o in -a -b -c -d -e -f -g -h -k -n -o -p -r -s -t -u -v -w -x -z \
         -G -L -N -O -R -S; do
  for x in f d nope ''; do
    eval "[[ $o \"\$x\" ]]"; c=$?
    eval "[ $o \"\$x\" ]"; s=$?
    if [ "$c" = "$s" ]; then echo "  agree($c) $o [$x]"; else echo "  DIFFER cond=$c test=$s $o [$x]"; fi
  done
done

echo "=== the file-kind, mode-bit, ownership and freshness primaries"
t '[[ -b f ]]'; t '[[ -c f ]]'; t '[[ -p f ]]'; t '[[ -S f ]]'
t '[[ -u f ]]'; t '[[ -g f ]]'; t '[[ -k f ]]'
t '[[ -O f ]]'; t '[[ -G f ]]'; t '[[ -O nope ]]'
t '[[ -N f ]]'; t '[[ -N nope ]]'

echo "=== -a is the primary when it leads and a connective when it joins"
t '[[ -a f ]]'; t '[[ -a d ]]'; t '[[ -a nope ]]'
t '[ -a f ]'
( eval '[[ x -a y ]]' ) 2>&1; echo "  st=$?"
t '[ x -a y ]'

echo "=== -R is the attribute, not the target"
v=hello
declare -n nr=v
t '[[ -R nr ]]'; t '[[ -R v ]]'; t '[[ -R nope ]]'
t '[ -R nr ]'; t '[ -R v ]'
t '[[ -v nr ]]'; t '[[ -n $nr ]]'
declare -n dangling=missing
t '[[ -R dangling ]]'; t '[[ -v dangling ]]'

echo "=== -v still reaches an array element, and -o a shell option"
declare -a A=(1 2)
t '[[ -v A ]]'; t '[[ -v A[1] ]]'; t '[[ -v A[9] ]]'
t '[[ -o errexit ]]'; t '[[ -o nosuchopt ]]'

echo "=== outside the table: a syntax error here, an operand there"
( eval '[[ -q x ]]' ) 2>&1; echo "  st=$?"
( eval '[ -q x ]' ) 2>&1; echo "  st=$?"
( eval '[[ -1 x ]]' ) 2>&1; echo "  st=$?"

echo "=== the spelling written is the spelling reprinted"
q() { [[ -h f ]] && [[ -L f ]] && [[ -R nr ]] && [[ -N g ]] && [[ -a f ]]; }
declare -f q

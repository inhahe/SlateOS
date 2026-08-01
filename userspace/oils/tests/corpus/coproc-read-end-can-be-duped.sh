# `${NAME[0]}` is a descriptor like any other, so `exec {v}<&"${NAME[0]}"` may
# alias it onto another number — which is how a script keeps a coproc readable
# after `NAME` is gone, or hands it to something that wants a fixed fd.
#
# A dup names one open file description, so the two numbers share a position:
# a line read through the alias is a line the original does not see again. That
# has to hold through the shell's own buffering as well as the OS pipe, since
# a buffered reader has already taken bytes off the pipe that neither number
# has handed out yet.
#
# Every body ends in a `read` that is fed only once the section is finished with
# the endpoints. A body that exited earlier would be disposed of at the next
# command boundary — `NAME` unset, both descriptors closed, see
# `coproc-is-disposed-of-when-it-is-reaped.sh` — and whether that had happened
# yet would be a race rather than something the script decides.
echo "=== two numbers, one position"
coproc F { echo ff; echo gg; read -r _; }
exec {v}<&"${F[0]}"
read -r a <&"$v";      echo "a=[$a]"
read -r b <&"${F[0]}"; echo "b=[$b]"
echo bye >&"${F[1]}"
wait "$F_PID"; echo "waitF=$?"
exec {v}<&-

echo "=== the alias outlives the array it came from"
coproc H { echo h1; echo h2; read -r _; }
hpid=$H_PID hw=${H[1]}
exec {w}<&"${H[0]}"
unset H
read -r c <&"$w";  echo "c=[$c]"
read -r -u "$w" d; echo "d=[$d]"
echo bye >&"$hw"
wait "$hpid"; echo "waitH=$?"
exec {w}<&-

echo "=== and an explicit number is the same thing"
coproc J { echo j1; read -r _; }
exec 7<&"${J[0]}"
read -r -u 7 f; echo "f=[$f]"
exec 7<&-
echo bye >&"${J[1]}"
wait "$J_PID"; echo "waitJ=$?"

# `${NAME[0]}` is a descriptor like any other, so `exec {v}<&"${NAME[0]}"` may
# alias it onto another number — which is how a script keeps a coproc readable
# after `NAME` is gone, or hands it to something that wants a fixed fd.
#
# A dup names one open file description, so the two numbers share a position:
# a line read through the alias is a line the original does not see again. That
# has to hold through the shell's own buffering as well as the OS pipe, since
# a buffered reader has already taken bytes off the pipe that neither number
# has handed out yet.
echo "=== two numbers, one position"
coproc F { echo ff; echo gg; }
exec {v}<&"${F[0]}"
read -r a <&"$v";      echo "a=[$a]"
read -r b <&"${F[0]}"; echo "b=[$b]"
wait "$F_PID"; echo "waitF=$?"
exec {v}<&-

echo "=== the alias outlives the array it came from"
coproc H { echo h1; echo h2; }
exec {w}<&"${H[0]}"
unset H
read -r c <&"$w";  echo "c=[$c]"
read -r -u "$w" d; echo "d=[$d]"
wait "$H_PID"; echo "waitH=$?"
exec {w}<&-

echo "=== and an explicit number is the same thing"
coproc J { echo j1; }
exec 7<&"${J[0]}"
read -r -u 7 f; echo "f=[$f]"
exec 7<&-
wait "$J_PID"; echo "waitJ=$?"

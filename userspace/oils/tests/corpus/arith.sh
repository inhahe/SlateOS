# Arithmetic evaluation, including the C-style for loop and (( )) status.
i=0
for (( j = 0; j < 3; j++ )); do i=$(( i + j )); done
echo "i=$i"
echo "$(( 7 / 2 )) $(( 7 % 2 )) $(( 2 ** 10 )) $(( 1 << 4 ))"
(( 0 )) && echo no
(( 1 )) && echo yes
n=5; (( n++ )); echo "n=$n"
echo "$(( 1 == 1 ? 10 : 20 ))"

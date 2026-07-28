# The commonly-used ${…} forms, including nesting and defaults.
v=hello
echo "${v}-${v:1:3}-${#v}"
echo "${v/l/L}|${v//l/L}|${v^^}|${v,,}"
echo "${unset:-def}|${unset:+set}|${v:+yes}"
p=/a/b/c.txt
echo "${p##*/}|${p%%.*}|${p#*/}|${p%/*}"
arr=(one two three)
echo "${arr[1]}|${#arr[@]}|${arr[*]}|${arr[@]: -1}"

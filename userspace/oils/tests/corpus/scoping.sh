# Dynamic scoping: a `local` in a caller is visible to its callees, is restored
# on return, and `local` with no value shadows with an *unset* variable.
v=global
show() { echo "  show sees v=[$v] set=${v+yes}"; }
mid()  { show; }
outer() {
  local v=outer
  mid
  local -a arr=(p q)
  echo "  outer arr=${arr[*]}"
}
outer
echo "after v=[$v]"

blank() {
  local v
  echo "  blank sees v=[$v] set=${v+yes}"
  v=assigned
  show
}
blank
echo "after-blank v=[$v]"

# `unset` inside a function removes the *local* binding, exposing the global.
hide() {
  local v=local
  unset v
  echo "  hide sees v=[$v]"
}
hide
echo "after-hide v=[$v]"

# A plain assignment inside a function without `local` writes the global.
leak() { w=fromfunc; }
leak
echo "leaked w=[$w]"

# Prefix assignments on a function call are scoped to that call only.
p=outer
peek() { echo "  peek p=[$p]"; }
p=temp peek
echo "after-prefix p=[$p]"

# Nested locals restore in LIFO order.
depth2() { local v=two; echo "  depth2 v=$v"; }
depth1() { local v=one; depth2; echo "  depth1 v=$v"; }
depth1
echo "final v=[$v]"

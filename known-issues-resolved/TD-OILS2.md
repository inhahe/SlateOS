### TD-OILS2. `osh` arrays — RESOLVED 2026-07-18 (associative arrays, negative/arith subscripts, subscript+operator combos, sparse indexed arrays, negative-index assignment targets, and associative subscripts inside `(( … ))` all implemented)

**Where:** `userspace/oils/src/parser.rs` (`split_name_subscript`,
`try_assignment`, `spanning_subscript_assignment`, `parse_array_elem`,
`is_declaration_command`), `userspace/oils/src/interp.rs`
(`apply_assignment`, `exec_declare_with_arrays`, `expand_array_ref`,
`array_element`, `assoc_element`, `array_elements`, `array_keys`,
`builtin_declare`, `VarLookup::get`).

**What:** Indexed arrays (`a=(x y z)`, `a[i]=v`, `a+=(w)`/`a+=str`,
keyed/sparse literals `a=([2]=x y)`, `${a[i]}`, `${a[@]}`/`${a[*]}`,
`${#a[@]}`/`${#a[i]}`, `${!a[@]}` indices, `unset a[i]`/`unset a`) **and**
associative arrays (`declare -A m`/`typeset`/`local`, `m[key]=v`,
`m=([k]=v …)`, the combined one-liner `declare -A m=([k]=v)` /
`declare -a a=(x y)`, `${m[key]}`, `${m[@]}`/`${m[*]}` values, `${!m[@]}`
keys, `${#m[@]}`, `unset m[key]`; insertion-ordered, string subscripts)
are implemented. Quoted `"${a[@]}"`/`"${!a[@]}"` keep one field per
element; unquoted forms field-split. Remaining deferred pieces:
1. ~~**Negative indices** (`${a[-1]}` = last element) return empty.~~
   **DONE 2026-07-18:** `array_element` resolves a negative subscript
   from the end via `resolve_index` (`-1` = last; a scalar acts as a
   one-element array); out-of-range negatives yield empty. Reads only —
   negative index in an *assignment target* (`a[-1]=v`) is still TODO.
2. ~~**Arithmetic subscripts inside `(( … ))`** (`(( a[i] + 1 ))`) are not
   recognized.~~ **DONE 2026-07-18:** the arith parser recognizes
   `name[expr]` (subscript is itself an arithmetic expression, so
   `a[i+1]` and negative `a[-1]` work) via the new defaulted
   `VarLookup::get_index`, which `Shell` implements over `array_element`.
   **Associative subscripts in `(( … ))` — DONE 2026-07-18:** the arith
   parser captures the raw bracketed subscript text (balanced brackets) and
   dispatches on array kind via the new `VarLookup::is_assoc`/`get_assoc`:
   an associative subscript (`m[foo]`, `m[$k]`) is the literal string key
   (not arith-evaluated), while an indexed subscript is still evaluated as
   an arithmetic expression. `Shell` implements both over `assoc`/
   `assoc_element`. Tests: `arith::associative_subscripts`,
   `interp::arith_associative_subscript`.
3. ~~**Subscript combined with an expansion operator**
   (`${a[i]:-default}`, `${a[@]#pat}`) is rejected at parse time.~~
   **DONE 2026-07-18:** the four operator variants (`ParamOp`,
   `ParamTrim`, `ParamSubstr`, `ParamReplace`) now carry an optional
   `index: Option<Box<Word>>`. The parser attaches a `[expr]` subscript to
   the operator (rejecting only `[@]`/`[*]` + operator, a bulk transform).
   `param_elem_value` resolves the element base value (associative key vs.
   arithmetic index, negatives from the end), and `${a[i]:=v}` writes the
   element back via `assign_elem`. All of `${a[i]:-def}`, `:+`, `:?`,
   `#`/`##`/`%`/`%%`, `:off:len`, and `/pat/repl` work per element.
4. ~~**Indexed arrays use a dense backing store** (`Vec<String>`), so a
   sparse literal (`a=([5]=x)`) fills gaps with empty elements.~~
   **DONE 2026-07-18:** `arrays` is now `HashMap<String, BTreeMap<usize,
   String>>` — sparse by construction. `a=([5]=x)` stores one element;
   `${#a[@]}` counts only assigned elements; `${!a[@]}` lists only the
   assigned indices (ascending); `unset a[i]` removes just that index
   (leaves a gap, no shift-down); `${a}`/`${a[0]}` read index 0
   specifically; and a negative subscript counts back from
   `highest_index + 1` (bash semantics).

All previously-deferred pieces are now implemented. (Negative index in an
*assignment target*, `a[-1]=v`, is supported: it resolves from
`highest_index + 1`, so `a[-1]=Q` overwrites the last element.)

### TD-OILS-A-MODIFIERS-OPERAND-IS-EXPANDED-MORE-OFTEN-THAN-BASH-EXPANDS-IT. `${u/aaa/$(cmd)}` runs `cmd`, and `${a[@]/$(cmd)/Y}` runs it once per element — 2026-08-07 — ✅ FIXED 2026-08-07

**Where:** `userspace/oils/src/interp.rs` — `Shell::expand_dynamic_with`'s
`ParamReplace` / `ParamCase` / `ParamTrim` arms, and `Shell::bulk_elements` /
`Shell::apply_bulk_op`.

**What.** A modifier's operand words can have side effects, so *how many times*
they are expanded is observable. bash expands them exactly as often as it can
use them; osh expanded them unconditionally, and per element. Three rules, all
missing. Measured by counting how often a `$( … )` in each position runs:

```text
                                        bash   osh
unset, replace pattern                    0      1    ${u/$(c)aaa/Y}
unset, replace replacement                0      1    ${u/aaa/$(c)Y}
unset, case                               0      1    ${u^^$(c)}
unset, trim                               0      1    ${u#$(c)a}
empty (set), trim                         0      1    ${n#$(c)a}
empty (set), replace                      2      2    ${n/$(c)/$(c)}   (no change)
4-element array, bulk replace pattern     1      4    ${g[@]/$(c)p/Y}
4-element array, bulk trim                1      4    ${g[@]#$(c)p}
4-element array, bulk case                1      4    ${g[@]^^$(c)}
empty/unset array, any bulk op            0      0    (no change)
```

1. **A substitution and a case modification do nothing to an *unset*
   parameter**, and bash finds that out before looking at either operand:
   `parameter_brace_patsub` and `parameter_brace_casemod` both open with
   `if (value == 0) return ((char *)NULL);` (subst.c:9119, 9366), where `value`
   is the parameter's own text. The test is set-ness, **not** emptiness —
   `x=''` is a value and its operands do run.
2. **A trim turns on the value instead.** The `#`/`%` arm of
   `parameter_brace_expand` breaks on
   `value == 0 || *value == '\0' || temp == 0 || *temp == '\0'`
   (subst.c:10084) before reaching `parameter_brace_remove_pattern`, so an
   empty-but-*set* parameter short-circuits too.
3. **A bulk operator expands its operands once for the whole collection.**
   `parameter_brace_patsub` computes `pat` and `rep` and only then calls
   `array_patsub`, which walks the elements with those two. An empty or unset
   collection expands them no times at all (the `value == 0` guard again).

Beyond the run counts this is observable as a spurious diagnostic:
``${u/aaa/`fi`}`` printed a `command substitution:` syntax error in osh and
nothing in bash.

**Fixed by** three matching changes:

* `Shell::set_modifier_operand` — `modifier_operand` that answers `None` for an
  unset parameter instead of an empty string, so the `ParamReplace` and
  `ParamCase` arms can return before expanding anything. It still reports the
  unset parameter on the same path, so `set -u` ordering is untouched.
* The `ParamTrim` arm returns early when the value is empty, which covers unset
  as well.
* `ReadyBulkOp` — a `BulkOp` whose operand words are already expanded. The split
  is the point: `Shell::ready_bulk_op` expands them once, and only the result is
  walked over the elements by `Shell::apply_bulk_op`, which no longer needs
  `&mut self`. `bulk_elements` returns before building one when the collection
  is empty.

Corpus case:
`userspace/oils/tests/corpus/a-modifiers-operand-is-expanded-only-when-it-is-needed.sh`.

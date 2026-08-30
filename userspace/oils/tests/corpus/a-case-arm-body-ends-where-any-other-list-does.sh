# A `case` arm's body is a `compound_list` like any other, so a command in it
# that carries no separator may be followed only by something that *ends the
# arm*. Two commands abutting inside an arm is the same syntax error it is
# anywhere else, and it is blamed on the token that abuts — not on the `;;`
# further along.
#
# bash spells the arm out as `pattern ')' compound_list`, with
# `compound_list: newline_list list0 | newline_list list1`. The `list1` arm is
# what lets a body end with no trailing `;`/`&`/newline at all, which is why
# `{ echo x; } esac` parses. What may then follow is fixed by the productions
# that receive the arm: `;;`, `;&` or `;;&` (`case_clause_sequence`), or `esac`
# (`case_clause`). Nothing else — a `(`, a `)`, a second command — reduces.
#
# Each probe runs under `eval` in a subshell, since a syntax error otherwise
# abandons the rest of the script.

echo '=== a stray ( in an arm body is blamed on the ( itself'
# Not on the `;;`, which is where a paren miscounted as an *open* would surface.
( eval 'case a in a) echo x( y;; esac' ); echo "s=$?"
echo '=== the ) twin, for comparison'
( eval 'case a in a) echo x) y;; esac' ); echo "s=$?"
echo '=== and outside a case, so the arm is not special'
( eval 'echo x( y' ); echo "s=$?"
( eval '{ echo x( y; }' ); echo "s=$?"

echo '=== two compound commands abutting in an arm'
( eval 'case a in a) ( : ) ( : ) ;; esac' ); echo "s=$?"
( eval 'case a in a) { :; } { :; } ;; esac' ); echo "s=$?"

echo '=== a body ending with no separator at all is fine before an ender'
# `list1` needs no terminator, so the `}` may be the last thing in the arm.
( eval 'case a in a) { echo x; } esac' ); echo "s=$?"
( eval 'case a in a) ( echo y ) esac' ); echo "s=$?"
( eval 'case a in a) { echo z; } ;& esac' ); echo "s=$?"
( eval 'case a in a) { echo w; } ;;& esac' ); echo "s=$?"

echo '=== but a ) after such a body is not one of the enders'
( eval 'case a in a) { echo x; } ) ;; esac' ); echo "s=$?"
( eval 'case a in a) { echo x; }( y;; esac' ); echo "s=$?"

echo '=== esac after a simple command is a word, not an ender'
# It is not in command position there, so the arm never ends and the `case`
# runs off the end of the input.
( eval 'case a in a) echo x esac' ); echo "s=$?"

echo '=== a separator lets anything follow, reserved words included'
( eval 'case a in a) echo x
esac' ); echo "s=$?"
# `wait` so the background `echo` lands before the status line rather than
# racing it; the point of the probe is that the `&` ends the arm, not when the
# command it backgrounds gets to run.
( eval 'case a in a) echo x& esac'; wait ); echo "s=$?"
( eval 'case a in a) echo x; esac' ); echo "s=$?"

echo '=== words that only look reserved are arguments'
( eval 'case a in a) echo x fi done then do } y;; esac' ); echo "s=$?"

echo '=== the same rule inside a nested case'
( eval 'case a in a) case b in b) echo n( m;; esac;; esac' ); echo "s=$?"
( eval 'case a in a) case b in b) { echo ok; } esac esac' ); echo "s=$?"

echo '=== a multi-line arm blames the line the ( is on'
( eval 'case a in
a)
  echo x( y
  ;;
esac' ); echo "s=$?"

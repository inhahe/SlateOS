# `a-failed-extent-parse-consumes-the-rest-of-the-string.sh` says a failed
# `xparse_dolparen` read stops at the end of the string. That is what a one-line
# body looks like, but it is not the rule. The rule is the *reader*.
#
# bash's string reader fills `shell_input_line` one line at a time, and charges
# `line_number++` for each one it fetches (parse.y:2361). A syntax error is
# therefore found — and reported, and echoed — on the line the reader is holding,
# with everything past that line's newline never looked at. `parse_string` leaves
# `bash_input.location.string` just there, and `xparse_dolparen` reads it back:
#
#     if (ep[-1] != ')')
#       { while (ep > ostring && ep[-1] == '\n') ep--; }
#     nc = ep - ostring;
#     *indp = ep - base - 1;
#     ret = (nc == 0) ? "" : substring (ostring, 0, nc - 1);   /* parse.y:4338-4377 */
#
# One `ep` gives both halves: the child is handed everything up to one byte short
# of it, and the caller's scan resumes *on* it. So a body that fails on its first
# line hands the child that line less a byte and leaves the rest of the string to
# be expanded as if the substitution had never been there — which is why the
# `)`s and the text after them come back out.
#
# There is no here-document special case in any of this. A here-document only
# matters because gathering its body is what advances the reader, so the lines it
# ate are counted.
#
# Only reachable under `no_longjmp_on_fatal_error` — a prompt expansion — as
# everywhere else the read's `jump_to_top_level (DISCARD)` abandons the command.
#
# Verified against bash 5.2.37.

echo "=== the read stops on the reader's line, not at the end of the string"
a='A$(fi
echo x
)B';                printf '1 [%s]\n' "${a@P}"
# The child is that line less a byte: `fi` becomes `f`.
b='A$(echo one
fi
echo x
)B';                printf '2 [%s]\n' "${b@P}"

echo "=== and what is left over is expanded, not copied"
y=Y
c='A$(fi
echo ${y}
)B';                printf '3 [%s]\n' "${c@P}"
# Recursive: the leftover's own `$(` fails its own read and leaves its own.
d='A$(fi
$(fi
)x
)B';                printf '4 [%s]\n' "${d@P}"

echo "=== a here-document is not a special case, only more lines for the reader"
e='A$(cat <<E
p
q
E
fi
)B';                printf '5 [%s]\n' "${e@P}"

echo "=== a body that runs the string out is the same rule with nothing left"
f='A$(fi
echo x';            printf '6 [%s]\n' "${f@P}"
g='A$(fi)B';         printf '7 [%s]\n' "${g@P}"

echo "=== the leftover is the string's, so a stray ) comes back with it"
h='A$(fi
q)r}B';             printf '8 [%s]\n' "${h@P}"

echo "=== a bracket arithmetic hands its leftover back to the expression"
i='A$[1+$(fi
echo x
)]B';               printf '9 [%s]\n' "${i@P}"

echo "=== a brace scan that stops part way still finds its }"
j='A${y:-p$(fi
q)r}B';              printf '10 [%s]\n' "${j@P}"

echo "=== done"

### TD-OILS-CMDSUB-BODY-RESERIALISED. bash re-serialises a `$( )` body without its trailing `;`, then rejects the result — 2026-07-28 — OPEN (bash bug; deliberately not matched)

**Where:** nothing in osh — this is a bash defect. Recorded so the divergence is
not mistaken for an osh bug, and so `tests/corpus/cmdsub-paren-terminator.sh`
stays honest about what it does *not* probe.

**What:** bash stores a parsed `$( … )` body as reconstructed text, and the
reconstruction drops a `;` or newline that ends the body's last command:

```sh
$ bash -c 'f() { echo A$( !; )B; }; declare -f f'
f ()
{
    echo A$(! )B          # the `;' is gone
}
```

The stored text is then re-parsed at expansion time — as a substitution body
again, so with `)` as the following token (TD-OILS-CMDSUB-EOF-TERMINATOR). With
the terminator gone, `!` has nothing to stand on and bash reports a syntax
error for a program its own outer parse had accepted:

```sh
echo A$( !; )B          # bash: command substitution: line 2: syntax error near `)'   osh: AB
echo A$(
!
)B                      # same
echo A$( ( ! ; ) )B     # same — and `( ! ; )` on its own is fine in both shells
echo A$( !; echo x )B   # both: AxB — an *interior* `;' survives the round trip
```

The give-away that this is re-serialisation and not grammar: the error carries
bash's `command substitution:` prefix and a line number past the end of the
input (the same tell as TD-OILS-CMDSUB-ABORT-LINENO), it exits 1 rather than
failing the outer parse with 2/127, and the identical construct outside a
substitution — `( !; )`, `{ !; }` — is accepted.

**Decision:** not matched. osh accepts these, which is what the grammar says.
Emulating a lossy round trip we do not perform would mean rejecting programs on
purpose. Revisit only if a real script depends on bash's rejection.

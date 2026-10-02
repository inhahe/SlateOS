### TD-OILS-CMDSUB-RW-ON-STDOUT-DROPPED. bash opens a `1<> file` inside `$( … )` and then does not install it — 2026-08-01 — WONTFIX (bash bug, not replicated)

**Where:** nowhere in osh — this is a divergence osh chooses, not a defect.

**Reproduce** (bash 5.2.37, `target/dvscratch/t3/qc`):

```sh
printf 'one\ntwo\nthree\n' > c
r=$( { echo W; } 1<>c ); echo "r=[$r]"   # bash: r=[W], c unchanged
r=`{ echo W; } 1<>c`;    echo "r=[$r]"   # bash: r=[],  c = "W\ne\ntwo\nthree\n"
{ echo W; } 1<>c | cat                   # bash:        c = "W\ne\ntwo\nthree\n"
```

Inside `$( … )` — and only there — bash performs the open (an unopenable path
still reports `No such file or directory` at redirection time and fails the
command) but leaves fd 1 pointing at the capture pipe. `{ read -r l <&1; }
1<>c` inside `$( … )` confirms it: `read: read error: 0: Bad file descriptor`,
which is what a dup of the *write-only* pipe answers. The same body in
backticks, in a pipeline stage, or at top level installs the descriptor
normally, and `3<>c 1>&3` works inside `$( … )` too — so this is an
inconsistency within bash rather than a rule about command substitution.

osh installs the descriptor in all four, which is the behaviour bash itself
gives in three of them. Named as deliberately absent in the header of
`tests/corpus/a-read-write-source-on-a-std-fd-keeps-one-offset.sh`.

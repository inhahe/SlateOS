## 734. `yesno` is one shared module with the answer source as a trait, rather than a third private copy — and `cp` carries it as `dyn` in the job

**Date:** 2026-08-30
**Lane:** B
**Decided by:** Claude (autonomous)

**In short:** Several of these utilities stop and ask a yes/no question — `rm -i`
deleting a file, `cp -i` overwriting one, `find -ok` before running a command.
*What counts as yes* has to be the same answer in all of them: a person who has
learned that `rm -i` accepts `yes` will type `yes` at `cp -i` too. There were
already two separate copies of that rule in this tree and they had already
drifted apart in a way a user could hit. Adding a third for `cp -i` would have
made it worse, so the rule is now one module (`coreutils::yesno`), and `cp`
holds the place its answers come from as a field in its job struct.

### The divergence that was already there

gnulib has exactly one of these (`lib/yesno.c`), called by `rm`, `cp`, `mv`,
`ln`, `install` and — through its own wrapper — `find -ok`. This tree had two:

| | how it read the line | what it did with a non-UTF-8 answer |
|---|---|---|
| `rm -i` | `read_until(b'\n')`, as bytes | accepted `y` normally |
| `find -ok` | `read_line` into a `String` | **failed the read and declined** |

The second row is a real bug, not a theoretical one. `read_line` returns
`InvalidData` for input that is not UTF-8, and `find`'s code treated a failed
read as end of input, which is a decline. A terminal in a single-byte locale
sends exactly that kind of byte, so a `y` typed at `find -ok` after a stray high
byte was refused where the same key at `rm -i` was accepted. Two copies of a
five-line rule had drifted within one crate; a third was not going to hold.

The module also puts the *locale* question in one place. Upstream's `rpmatch`
consults `LC_MESSAGES`, so a French `oui` is a yes under `fr_FR`. This tree has
one locale and implements the C one's `yesexpr` (`^[yY]`); when locales arrive,
there is now exactly one function that has to learn about them.

### The cost: a test double in the shipped library

`Canned` — a queue of answers for tests — is a `pub` type in the library, not
`#[cfg(test)]`. That is a real cost: it is API surface that exists only for
testing, and it ships.

It cannot be `#[cfg(test)]`, because the things that need it are the tests of
the *binaries*, and each binary is a separate compilation unit from the library.
A `cfg(test)` item in `lib.rs` is invisible to every one of them. The
alternative — a private `Canned` duplicated in each binary's test module — is
the same duplication this entry is about, one level down, and `rm`'s and `cp`'s
copies would drift in the same way the readers did.

Against that, a prompt is the part of these programs that most needs testing and
is least testable through a real terminal: the interesting cases are several
answers consumed by several prompts, and an input that ends part way through.
`cp -i a b c d` asks three questions of one stream, and "asked and was declined"
versus "never asked" are two behaviours that leave the same bytes on disk and
the same exit status — only a count of consumed answers tells them apart. A
trait was the way to get at that, and a trait with no test implementation in
reach is a trait nobody can use.

### Why `dyn` in `Job`, not a third type parameter

`cp`'s `Job` is already generic over its two output sinks, and eleven function
signatures name it. Adding `A: Answers` would have touched all eleven to serve
one call site. The field is `&'a mut dyn Answers` instead.

The usual objection to `dyn` on a hot path does not apply: the indirect call
happens once per *prompt*, which is once per human keypress. `rm` had already
made the same choice for the same reason, so the two agree.

### What was rejected

- **A free function that reads `io::stdin()`.** Simplest, and untestable except
  by spawning a process and driving its stdin — which is a different kind of
  test with a different failure mode, and would have made the eight `-i`
  behaviours below into eight process spawns.
- **Leaving `find -ok`'s copy alone and sharing only between `rm` and `cp`.**
  That fixes nothing: the bug is in the copy that would have been left.
- **Taking a `&mut dyn BufRead` instead of a purpose-named trait.** It reads
  more generally and says less: the contract is *one line per call, `None` at
  end of input, and end of input is indistinguishable from a failed read* —
  which is gnulib's contract, since `getline` returns `-1` for both — and a
  `BufRead` parameter states none of that.

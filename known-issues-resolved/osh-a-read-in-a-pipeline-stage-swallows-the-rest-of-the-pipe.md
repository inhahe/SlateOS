## osh: a `read` in a pipeline stage swallows the rest of the pipe

**Status:** fixed 2026-08-26 (found the same day while verifying the
special-redirection-filename fix; **not** caused by it — it reproduced with no
special filename involved). The fix went wider than the one proposed below, and
took two neighbouring defects with it — see *"What was actually done"* at the
end. Regression case:
`userspace/oils/tests/corpus/a-pipeline-stages-fd-0-is-an-ordinary-descriptor.sh`.

After `read` consumes a line from a pipeline stage's stdin, any *later command*
in that same stage sees EOF. Measured, bash 5.2.21 vs osh, glibc:

```text
                                                    bash        osh
printf 'A\nB\n' | { read -r l; /bin/cat; }          B           (nothing)
printf 'A\nB\n' | { read -r l; cat; }               B           (nothing)
printf 'A\nB\n' | { read -r l; sed 's/^/x/'; }      xB          (nothing)
printf 'A\nB\n' | { read -r l; head -1; }           B           (nothing)
printf 'A\nB\nC\n' | { read -r l; /bin/cat; }       B|C         (nothing)
printf 'P1\nP2\n' | { exec 3<&0; read -u 3 l
  ; cat < /dev/fd/3; }                              P2          (nothing)
```

Two neighbouring cases are *right*, which localises it precisely: two
successive `read`s work (`l=A m=B`), and so does `while read`. Both of those
stay inside the shell. Only handing the descriptor to a **child** loses data.

**Cause.** `StdinSrc::pipe` (interp.rs:3200) wraps the pipe's read end in
`io::BufReader::new(r)` — the default 8 KiB buffer. `read_record` peeks through
`fill_buf`, which drains up to 8 KiB of the pipe into that buffer; the child
then inherits the raw fd (`try_clone`, interp.rs:25933) and finds it already
empty. The bytes are not lost, they are sitting in the shell's buffer where
nothing will ever hand them back — a pipe has no `seek` to give them back with.

The shortcut is already acknowledged in a comment at the `try_clone` site
("Bytes already buffered by an earlier in-stage `read` are not replayed — a
rare edge case"), but `read header; cat` is not a rare idiom, and bash gets it
right.

**Fix.** The rule this needs is one osh has already written down, one type over.
`FileInput::build` (interp.rs:2506) sizes its buffer from seekability —
`FILE_INPUT_READAHEAD` when the descriptor can seek, **1 byte when it cannot** —
precisely because "without it `sync` cannot give the unconsumed remainder back,
so there must not be one." A pipe can never seek, so `StdinSrc::pipe` must use
`io::BufReader::with_capacity(1, r)`. That is also what bash does: `zsyncfd`
reads unseekable input one byte at a time so the descriptor is left exactly
after the delimiter. The cost — one `read(2)` per byte for `cmd | while read` —
is the same cost bash pays, and correctness here is the point of the exercise.

`take` (mapfile) and `read_to_end` are unaffected: both consume to EOF, and
`BufReader` overrides `read_to_end` to drain its buffer and then delegate.

**What was actually done.** Shrinking the buffer would have fixed the symptom
and left the cause, which was not the buffer size but the fact that a pipeline
stage's fd 0 was held as a *shape of its own* — `StdinSrc::Pipe`, a
`RefCell<BufReader<PipeReader>>` — rather than as an ordinary input descriptor.
Three defects followed from that one fact, and only the first is the one this
entry was filed for:

1. The stranded read-ahead above.
2. **`exec 3<&0` in a pipeline stage was a silent no-op.** The fd-0 lookup
   (`clone_input_fd`) only knows about `InputFd`s, so it found nothing to
   duplicate and fell back to the empty stream it gives an unbound stdin:
   `printf 'A\nB\n' | { exec 3<&0; read -u 3 l; }` left `l` empty with rc 0
   where bash reads `A`. The transient form `{ read -r l <&3; } 3<&0` had the
   same hole, through `plan_stdin_fd`.
3. **`/dev/stdin` on a pipe was answered by draining the pipe** into a byte
   snapshot rather than re-opening the descriptor.

So `StdinSrc::Pipe` was deleted outright. `StdinSrc::pipe` now builds
`StdinSrc::Fd(file_input(pipe_reader_into_file(r)))`, i.e. an
`InputSrc::File(FileInput)` over a pipe-derived `File` — at which point
`FileInput::build`'s existing seekability rule supplies the one-byte buffer for
free, a child gets a duplicate positioned where the shell is, and `/dev/stdin`
is a genuine `/proc/self/fd` re-open. Two supporting changes fell out:

* `FileInput` gained a `seekable` field and a `readable_now()`, so `read -t 0`
  still distinguishes a seekable file (always ready) from a live descriptor
  (must be probed) now that the distinction is no longer visible in the enum's
  shape. `pipe_reader_readable_now(&PipeReader)` became
  `file_readable_now(&File)`.
* `AmbientStdin` lost its `Opaque` variant (a stage's fd 0 is an
  `AmbientStdin::Fd` like any other) and gained a reader, `Shell::dup_input_fd`,
  which resolves fd 0 against the ambient rather than the `exec`-installed
  table. Both dup paths — persistent (`apply_persistent_dup_in`) and transient
  (`plan_stdin_fd`) — go through it, which is what closes defect 2.

The comment conceding the shortcut ("Bytes already buffered by an earlier
in-stage `read` are not replayed — a rare edge case") is gone with the code it
described.

One divergence in this area remains, and is **not** part of this: osh models
fds ≥ 3 in user space and does not pass them to children, so
`printf 'A\nB\n' | { exec 3<&0; ls -l /proc/self/fd/3; }` still fails where
bash succeeds. That is the separately-tracked user-space-fd-table limitation.

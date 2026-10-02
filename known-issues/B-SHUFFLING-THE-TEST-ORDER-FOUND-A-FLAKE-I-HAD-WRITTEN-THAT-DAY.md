## B-SHUFFLING-THE-TEST-ORDER-FOUND-A-FLAKE-I-HAD-WRITTEN-THAT-DAY (lane B, 2026-09-13)

**In short:** `posix`'s 20,745 tests pass every time in the default order. Run
them in a random order and one fails — a test I had added hours earlier, which
took a slot from a shared pool of eight and never gave it back.

### How it surfaced

`stdio.rs`'s `StdStreamTestGuard` documents a flake that was "26-in-30 under
`--shuffle` while the default alphabetical order hid it completely", so after
de-racing the stdio streams I ran the suite shuffled to check the fix. It found
something else:

    running 20745 tests (shuffle seed: 1789357352031162400)
    test sysv_msg::tests::msg_copy_without_nowait_is_einval ... FAILED
    assertion failed: qid >= 0

`msgget` had returned -1. The queue pool is `MAX_QUEUES` = **8**, thirty tests
allocate from it, and the five I wrote for `MSG_COPY` allocated **outside the
`with_clean()` helper every other test uses** — which takes a lock and resets
the pool. Mine leaked a slot each. The failing test is simply the one that
happened to ask ninth; under alphabetical order it asked earlier and passed.

### Two wrong fixes before the right one, for one reason

I changed the code twice before reading the failure message.

1. **An RAII `QueueReaper`** to release each slot on drop. It broke
   `test_msgget_pool_exhaustion_enospc`, which fills the pool deliberately and
   asserts the ninth `msgget` fails: my inserted `let _reap` landed **inside a
   loop body**, so each slot was freed on the next iteration and the pool never
   filled.
2. **A second lock** over the pool. Redundant — and still not the problem.

The actual message said `assertion left == right, left: 2293761, right: -1`,
which names the pool-exhaustion test and no other. **The file already had the
right mechanism** (`with_clean`, since before I arrived); five tests of mine
simply did not use it. The fix is five lines.

*Read the failure before changing the code* is not a new rule. It is worth
recording that I broke it while actively working through a backlog of
test-hygiene defects.

### A real defect found along the way

`with_clean` took its lock with `.unwrap()`. When the pool-exhaustion test
panicked, the mutex poisoned and the next **seventeen** tests panicked inside
`.unwrap()` — one defect reported as eighteen failures, seventeen of them
pointing at innocent code. Now poison-tolerant, the same repair made in
`crt.rs`, `pthread.rs` and `stdio.rs` this session.

### Verified

The failing seed (1789357352031162400) now passes all 20,745, as do two further
random shuffles. **`--shuffle` is worth running after any test-ordering change**
and is not in any gate; the default order is a single sample of one
permutation out of 20,745!.

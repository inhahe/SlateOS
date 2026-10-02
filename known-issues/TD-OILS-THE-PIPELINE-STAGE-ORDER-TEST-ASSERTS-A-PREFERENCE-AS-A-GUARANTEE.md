### TD-OILS-THE-PIPELINE-STAGE-ORDER-TEST-ASSERTS-A-PREFERENCE-AS-A-GUARANTEE — 2026-08-08 — OPEN (accepted)

**Where:** the test `a_pipelines_stages_begin_in_pipeline_order`
(`userspace/oils/src/interp.rs`, search the name), against the start handshake
in `Shell::exec_threaded_pipeline` — `start_wait` and the two `recv_timeout`
calls that use it.

**What.** Under a machine deliberately oversubscribed — four concurrent copies
of the whole 1385-test binary — the test failed once. Every assertion in it
pins an exact trace order (`"+ true\n+ :\n"`, and `starts_with("+ f\n")` for
the function case), and any of them could have been the one; **the assertion
text was not captured**, so which one it was is unknown.

**Why it can happen at all, from the code.** The handshake is two chains. The
first — "the stage's thread has begun" — is unbounded and cannot lapse. The
second — "the stage has traced its first command and is about to run it" — is
bounded by `start_wait`, **100 ms**, and a lapsed wait *runs the successor
anyway*. That bound is deliberate and is documented at its definition: a stage
that blocks before its first command (`echo $(cat) | cat` with no input) must
not be able to hold the whole pipeline up, and no arrangement of stages may
deadlock. So the ordering is offered as a strong preference, not a lock —
which is also all bash offers, its stages being concurrent processes.

That leaves exactly one path in the code by which the trace can come out
reversed: the predecessor did not reach `xtrace_command` within 100 ms of its
successor starting to wait. The signal is sent *after* the trace is written
(see the comment above `signal_stage_started` at its call site), so no
reordering is possible while the wait holds. Under 4× oversubscription a thread
simply not being scheduled for 100 ms is plausible — **but this was not
measured, and is a candidate, not a finding.**

**Measured 2026-08-08.** One occurrence in 40 contended full-suite runs. Then:

- 48 further contended full-suite runs (12 iterations × 4 concurrent copies):
  **no occurrence.**
- 960 targeted runs of this test alone (8 concurrent processes × 120): **no
  occurrence.**

So the observed rate is 1 in ~1050 runs overall, and it has not been seen since
the run that produced it.

**Proper fix.** None to the implementation: hardening the handshake into a real
lock would deadlock the very pipelines the bound exists for, and would promise
an ordering bash itself does not. The mismatch is that the test asserts as a
guarantee what the implementation offers as a bounded preference. Leave both as
they are — the assertion is the right thing to test on a machine that is not
saturated, and saturation is not a configuration osh has to be correct under.
If it is ever seen again, **capture the whole failure first** (test name,
assertion, `left`/`right`); if the failure is the `starts_with` one, that
confirms the lapsed-wait path and the entry can be closed as by-design.

**Impact.** A red run under artificial oversubscription only. It has never been
seen in a normal `cargo test -p oils --lib`, which is the gate.

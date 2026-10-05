### TD-OILS-THE-ARGUMENT-STACK-BASE-FRAME-WAS-CAPTURED-ON-EVERY-TURN-ON. A function that toggled `extdebug` grew `BASH_ARGC` by one every time — 2026-08-11 — FIXED 2026-08-11

**Where:** `userspace/oils/src/interp.rs`, the `shopt` builtin's OFF→ON
transition. It seeded a base frame on *every* such transition, where bash seeds
one on the first and none after.

```sh
$ shopt -s extdebug; f() { shopt -u extdebug; shopt -s extdebug; }; f; f; f
$ echo "[${BASH_ARGC[*]}]"
bash: [0]
osh : [0 0 0 0]
```

**Why bash does it.** The turn-on handler calls `init_bash_argv`, whose whole
body is under a once-only flag:

```c
  void init_bash_argv (void)
  { if (bash_argv_initialized == 0) { save_bash_argv (); bash_argv_initialized = 1; } }
```

The guard is load-bearing rather than thrifty. A call pops only the frame its
own call pushed — osh records that per frame in [`Shell::arg_frame_pushed`],
so toggling mid-call cannot desynchronise the two stacks — which means a
re-seeded base has nothing that will ever remove it. Every off/on round inside
a function left one more behind, and the stack grew without bound.

The base is a snapshot besides: a later `set --` does not move it, and turning
extdebug *off* clears nothing. What being off does is skip the push, so a call
made while it is off contributes no frame to find afterwards.

**Fixed** in this commit. New field [`Shell::bash_argv_initialized`], set at the
first seeding and inherited by subshells with the stacks themselves.

Corpus: `the-argument-stack-base-frame-is-captured-once-and-never-again.sh`.
Unit test: `the_argument_stack_base_frame_is_captured_once_and_never_again`.

**How it was found:** a `declare -p | grep BASH_ARGC` row in the corpus for
TD-OILS-AN-EMPTY-ARGUMENT-STACK-ARRAY-LISTED-AS-MERELY-DECLARED came back with
one frame too many.

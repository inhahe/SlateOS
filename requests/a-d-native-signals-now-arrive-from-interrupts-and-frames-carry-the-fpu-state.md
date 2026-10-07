# A → D — native signals now arrive from interrupts, and frames carry the FPU state

**Filed:** 2026-10-07 by lane A. **Status:** OPEN (for lane D to read; no
change is required for anything to keep working). Design: design-decisions
§1541.

## In short

Until today the kernel delivered a signal only when a system call returned,
so a native program computing in a loop never ran its `SIGINT` handler. Now
it also delivers when an interrupt returns to the program -- the next timer
tick -- as Linux does. Two consequences for the C library, neither needing a
change, both worth knowing:

1. **A handler can now interrupt any instruction**, not only a system call.
   The trampoline is entered exactly as before (`rdi` = signal, `rsi` =
   `&SignalContext`, `rsp` ≡ 8 mod 16), and `SYS_SIGNAL_RETURN` now puts
   back *every* register, `rcx` and `r11` included, plus the FPU state.
2. **The frame is larger.** Above the context (and above the siginfo tail,
   for a trampoline registered with `SIGNAL_FRAME_SIGINFO`) the kernel now
   keeps `SignalFrameExt` -- 32 bytes: a magic number, `rcx`, `r11`, a length
   -- and then the FPU image (the XSAVE area: about 1 KiB with AVX, 2.7 KiB
   with AVX-512). Every offset you read is unchanged: the context is still at
   `rsi`, the tail still at `rsi + 136`. But an **alternate signal stack**
   now needs room for the image: a `MINSIGSTKSZ` of 2048 is too small on an
   AVX-512 machine, and a frame that does not fit is not delivered (the
   signal stays pending). Linux has the same growth and tells programs
   through `AT_MINSIGSTKSZ`; the native equivalent would be for the library
   to size `sigaltstack` buffers from `SIGSTKSZ` (8192) or larger.

Also changed, same day:

- **A handler starts with the initial FPU state** (MXCSR 0x1F80, x87
  defaults, vector registers zero), as on Linux, and the interrupted state is
  restored on return. A handler that wanted to read the interrupted code's
  MXCSR or vector registers could not before either (they were simply live
  and unprotected).
- **The frame is built below the interrupted code's 128-byte red zone**,
  which it was overwriting.
- An SEH-style exception frame (`ExceptionContext`) gets the same extension
  above it and the same full restore on `SYS_EXCEPTION_RETURN` -- but its
  handler still sees the live FPU state, so an SSE fault's handler can read
  MXCSR's flags.
- `SIGKILL` and `SIGSTOP` never reach the trampoline (a POSIX timer can now
  queue either; they take their kernel action instead).

## Nothing to do unless

your trampoline or `sigaltstack` code assumes the frame ends right after the
context or tail, or allocates a signal stack smaller than about 4 KiB.

-- lane A

/*
 * ctest-initfini — ring-3 regression test for the sysroot's ELF
 * constructor/destructor walks: `.preinit_array`, `.init_array` and
 * `.fini_array`.
 *
 * Closes the "no in-tree consumer" half of `known-issues.md` →
 * D-CRT-INIT-ARRAY, and answers
 * `requests/a-b-crt-init-array-consumer-must-be-c-not-cpp.md`.
 *
 * ## Why this is C and not C++, which is the whole point of the request
 *
 * Lane A measured both shapes with our own codegen flags and found:
 *
 *     consumer                                      .init_array  .fini_array
 *     C++, global object with a destructor          1 entry      SECTION ABSENT
 *     C, __attribute__((constructor))/((destructor)) 1 entry      1 entry
 *
 * A C++ program never populates `.fini_array`: its destructors are registered
 * at run time by `__cxa_atexit` and run by `__cxa_finalize`.  That is the C++
 * ABI, not a zig or musl artifact.  So a C++ fixture would exercise the
 * `.init_array` walk, leave the `.fini_array` walk exactly as unproven as it
 * was, and let an entry naming the two together read as closed.  The likeliest
 * first consumer is C++ (Oils/YSH), so that is the probable path rather than a
 * corner case.
 *
 * `build.py` refuses to emit this fixture if the linked ELF is missing either
 * array, so switching this translation unit to C++ later fails the build
 * instead of silently halving what it proves.
 *
 * ## What is actually being tested, and why a fixture rather than a unit test
 *
 * `posix/src/crt.rs` has four host unit tests over `run_init_array` and
 * `run_fini_array`.  They pass arrays the test itself built, to functions the
 * test itself called.  Every one of them would still pass if the *linker*
 * never defined `__init_array_start`, if `__libc_start_main` never called
 * `run_constructors`, or if `atexit(run_destructors)` were dropped — because
 * none of those three things is a function a host test can reach.  The weak
 * boundary symbols resolve to null on a host build by construction.
 *
 * The walk has therefore only ever been proven to be a correct *no-op*.  This
 * fixture is the first program in the tree whose boundary symbols are
 * non-null, so it is the first time the non-null path executes at all.
 *
 * ## The exit codes, which are the point of the design
 *
 * A walk that does not happen and a walk that happens correctly must not
 * produce the same observation.  The codes below are chosen so that each
 * failure names itself:
 *
 *     42  every walk ran, in the specified order.  The only success.
 *      7  constructors ran, main ran, and NOTHING overrode main's status —
 *         i.e. the `.fini_array` walk never executed.  This is the C++ shape,
 *         and it is the code this fixture exists to be able to return.
 *      8  `.init_array` did not run: main was entered with no constructor
 *         having recorded anything.
 *      9  `.init_array` ran but `.preinit_array` did not.  Split from 8
 *         because they are separate walks over separate sections with
 *         separately synthesised boundary symbols, and one can work while the
 *         other does not.
 *     31  the first recorded event was not the preinit entry.
 *     32  the constructors ran out of order (ascending order is the ABI).
 *     33  the sequence has the wrong length.
 *     34  the destructors ran in constructor order rather than reversed.
 *     30  some other sequence mismatch.
 *
 * Note what 7 requires: main deliberately exits with a FAILING status, and
 * only a destructor can turn it into 42.  Success is therefore something the
 * `.fini_array` walk has to actively produce.  If the walk is missing, the
 * fixture cannot accidentally pass — which is the property that a fixture
 * whose main returned 42 directly would not have.
 *
 * ## Why `write` and `_exit` rather than `printf` and `return`
 *
 * The verdict is reached inside a destructor, i.e. from within `exit()`'s
 * atexit chain.  Calling `exit()` again there is undefined, so the verdict
 * uses `_exit`, which means stdio buffers are never flushed.  Every diagnostic
 * therefore goes out through an unbuffered `write(1, ...)` as it happens, so
 * the serial log carries the observed order even in the runs that fail.
 */

#include <stddef.h>
#include <sys/types.h>
#include <unistd.h>

/* --- exit codes (mirrored in kernel/src/proc/spawn.rs's self-test legend) -- */

#define EXIT_ALL_PASSED        42
#define EXIT_NO_FINI_WALK       7
#define EXIT_NO_INIT_WALK       8
#define EXIT_NO_PREINIT_WALK    9
#define EXIT_SEQ_OTHER         30
#define EXIT_SEQ_NO_PREINIT    31
#define EXIT_SEQ_CTOR_ORDER    32
#define EXIT_SEQ_LENGTH        33
#define EXIT_SEQ_DTOR_ORDER    34

/* --- the event trail ------------------------------------------------------ */

enum {
    EV_PREINIT   = 1,
    EV_CTOR_LOW  = 2,
    EV_CTOR_HIGH = 3,
    EV_MAIN      = 4,
    EV_DTOR_HIGH = 5,
    EV_DTOR_LOW  = 6
};

#define EV_COUNT 6
#define SEQ_MAX  16

/*
 * `volatile` so the compiler cannot decide the trail is unobservable and fold
 * the whole thing away: from its point of view nothing outside this file ever
 * calls the constructors, and every store below is dead.
 */
static volatile unsigned char g_seq[SEQ_MAX];
static volatile unsigned      g_n;

/* --- unbuffered output ---------------------------------------------------- */

static void emit(const char *s)
{
    size_t n = 0;
    while (s[n] != '\0') {
        n++;
    }
    if (n != 0) {
        ssize_t written = write(1, s, n);
        (void)written; /* a failed diagnostic must not change the verdict */
    }
}

static void emit_u(unsigned v)
{
    char buf[12];
    size_t i = sizeof buf;

    buf[--i] = '\0';
    if (v == 0) {
        buf[--i] = '0';
    }
    while (v != 0) {
        buf[--i] = (char)('0' + (v % 10u));
        v /= 10u;
    }
    emit(&buf[i]);
}

static void emit_seq(void)
{
    unsigned i;

    emit("[initfini] observed order:");
    for (i = 0; i < g_n && i < SEQ_MAX; i++) {
        emit(" ");
        emit_u((unsigned)g_seq[i]);
    }
    emit("\n");
}

static void record(unsigned char ev, const char *name)
{
    if (g_n < SEQ_MAX) {
        g_seq[g_n] = ev;
        g_n = g_n + 1u;
    }
    emit("[initfini] ");
    emit(name);
    emit("\n");
}

/* --- the verdict ---------------------------------------------------------- */

/*
 * Reached from the destructor that is expected to run LAST.  If the reverse
 * walk is wrong that destructor runs first instead, the trail is short, and
 * the length check below refuses — which is the refusal half of the two-probe
 * rule reached through the fixture's own logic rather than through a comment.
 */
static void verdict(void)
{
    static const unsigned char want[EV_COUNT] = {
        EV_PREINIT, EV_CTOR_LOW, EV_CTOR_HIGH, EV_MAIN, EV_DTOR_HIGH, EV_DTOR_LOW
    };
    unsigned i;

    emit_seq();

    if (g_n != (unsigned)EV_COUNT) {
        emit("[initfini] FAIL: expected 6 events\n");
        _exit(EXIT_SEQ_LENGTH);
    }
    for (i = 0; i < (unsigned)EV_COUNT; i++) {
        if (g_seq[i] == want[i]) {
            continue;
        }
        if (i == 0) {
            emit("[initfini] FAIL: .preinit_array entry did not run first\n");
            _exit(EXIT_SEQ_NO_PREINIT);
        }
        if (i == 1 || i == 2) {
            emit("[initfini] FAIL: .init_array ran out of ascending order\n");
            _exit(EXIT_SEQ_CTOR_ORDER);
        }
        if (i == 4 || i == 5) {
            emit("[initfini] FAIL: .fini_array did not run in reverse order\n");
            _exit(EXIT_SEQ_DTOR_ORDER);
        }
        emit("[initfini] FAIL: unexpected event order\n");
        _exit(EXIT_SEQ_OTHER);
    }

    emit("[initfini] PASS: preinit, init and fini arrays all walked in order\n");
    _exit(EXIT_ALL_PASSED);
}

/* --- .preinit_array ------------------------------------------------------- */

/*
 * No attribute spells "preinit constructor", so the function pointer is placed
 * in the section by hand.  `used` keeps it past -O2; the array is otherwise
 * referenced by nothing.
 */
static void preinit_entry(void)
{
    record(EV_PREINIT, "preinit_array entry ran");
}

__attribute__((used, section(".preinit_array")))
static void (*const g_preinit)(void) = preinit_entry;

/* --- .init_array ---------------------------------------------------------- */

/*
 * Two entries with explicit priorities, because one entry cannot distinguish
 * "walked the array" from "ran the single thing it found".  Lower priority
 * runs first, so 101 before 102, and `run_init_array` walks ascending.
 */
__attribute__((constructor(101)))
static void ctor_low(void)
{
    record(EV_CTOR_LOW, "init_array ctor priority 101 ran");
}

__attribute__((constructor(102)))
static void ctor_high(void)
{
    record(EV_CTOR_HIGH, "init_array ctor priority 102 ran");
}

/* --- .fini_array ---------------------------------------------------------- */

/*
 * `CTEST_INITFINI_NO_FINI` is the negative control, and it is not a debugging
 * leftover: `build.py` compiles this file a second time with it defined, and
 * requires its own "both arrays are present" check to REFUSE the result.  A
 * check that has never been seen to fail is a check nobody has tested.
 *
 * With it defined, the destructor functions are still emitted and still
 * referenced by the symbol table — `used` sees to that — and no `.fini_array`
 * section exists at all.  That is precisely the C++ shape lane A measured, so
 * the negative control is the real failure rather than an imitation of it.
 */
#ifdef CTEST_INITFINI_NO_FINI
#define CTEST_DESTRUCTOR(pri) __attribute__((used))
#else
#define CTEST_DESTRUCTOR(pri) __attribute__((destructor(pri)))
#endif

/*
 * Matching priorities on the destructors, so their `.fini_array` slots sit in
 * the same relative order as the constructors' `.init_array` slots.  The
 * reverse walk then has to yield 102 before 101; anything else means
 * `run_fini_array` is not walking backwards, which is a real bug that a
 * single-destructor fixture could not see.
 */
CTEST_DESTRUCTOR(102)
static void dtor_high(void)
{
    record(EV_DTOR_HIGH, "fini_array dtor priority 102 ran");
}

CTEST_DESTRUCTOR(101)
static void dtor_low(void)
{
    record(EV_DTOR_LOW, "fini_array dtor priority 101 ran");
    verdict();
}

/* --- main ----------------------------------------------------------------- */

int main(void)
{
    unsigned before = g_n;

    if (before == 0u) {
        emit("[initfini] FAIL: main entered with no constructor having run\n");
        return EXIT_NO_INIT_WALK;
    }
    if (g_seq[0] != (unsigned char)EV_PREINIT) {
        emit_seq();
        emit("[initfini] FAIL: .init_array ran but .preinit_array did not\n");
        return EXIT_NO_PREINIT_WALK;
    }
    if (before != 3u) {
        emit_seq();
        emit("[initfini] FAIL: expected 3 events before main\n");
        return EXIT_SEQ_LENGTH;
    }

    record(EV_MAIN, "main ran");

    /*
     * Deliberately a failing status.  Only `dtor_low` can turn it into 42, so
     * a missing `.fini_array` walk shows up as 7 rather than as a pass.
     */
    emit("[initfini] main returning 7; only the fini walk can make this 42\n");
    return EXIT_NO_FINI_WALK;
}

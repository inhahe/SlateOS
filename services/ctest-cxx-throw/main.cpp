/*
 * ctest-cxx-throw -- a C++ exception thrown and caught on SlateOS, end to end.
 *
 * Guards known-issues.md ->
 * D-POSIX-DL-ITERATE-PHDR-NEVER-CALLED-BACK-SO-NO-CXX-THROW-COULD-BE-CAUGHT.
 *
 * ## What it is for
 *
 * A C++ program here links zig's libc++, libc++abi and libunwind in front of
 * this system's libc.a. libunwind finds the tables that unwind a stack --
 * `.eh_frame_hdr`, the `PT_GNU_EH_FRAME` segment -- by asking the C library's
 * `dl_iterate_phdr` for each loaded object, and has no other way. Until
 * 2026-09-29 that returned 0 without calling back, so libunwind found no
 * object, `_Unwind_RaiseException` reported the end of the stack, and every
 * `throw` ended in `std::terminate`: no C++ exception could be caught, and a
 * port that relies on one (cmake, most of all) could not run. The link had
 * always succeeded, which is exactly why nothing had noticed.
 *
 * posix's host tests check `dl_iterate_phdr` against a synthetic ELF image;
 * only a program on the target has a real one, a real thread pointer and a
 * real unwinder. This is that program.
 *
 * ## It cannot hang
 *
 * Nothing waits, reads or sleeps; every throw is caught in this function or
 * ends the process through std::terminate, which is a failed exit, not a
 * hang.
 *
 * Exit code 42 == every check passed; anything else names the first failing
 * check (see the `return`s below). 134 (SIGABRT) is std::terminate: an
 * exception nobody could catch -- the bug itself.
 */

#include <dlfcn.h>
#include <exception>
#include <link.h>
#include <stdexcept>
#include <string.h>

namespace {

struct Found {
    int calls = 0;
    int named_empty = 0;
    int holds_main = 0;
    int has_eh_frame_hdr = 0;
    ElfW(Addr) eh_frame_hdr = 0;
};

int record(struct dl_phdr_info *info, size_t size, void *data)
{
    Found *f = static_cast<Found *>(data);
    f->calls++;
    if (size < sizeof(struct dl_phdr_info))
        return 99;
    f->named_empty = info->dlpi_name != nullptr && info->dlpi_name[0] == '\0';
    const ElfW(Addr) here = reinterpret_cast<ElfW(Addr)>(&record);
    for (ElfW(Half) i = 0; i < info->dlpi_phnum; i++) {
        const ElfW(Phdr) &p = info->dlpi_phdr[i];
        const ElfW(Addr) start = info->dlpi_addr + p.p_vaddr;
        if (p.p_type == PT_LOAD && here >= start && here < start + p.p_memsz)
            f->holds_main = 1;
        if (p.p_type == PT_GNU_EH_FRAME) {
            f->has_eh_frame_hdr = 1;
            f->eh_frame_hdr = start;
        }
    }
    return 7;
}

int destroyed = 0;

struct Guard {
    ~Guard() { destroyed++; }
};

/* Deep enough that the unwinder walks many frames, each with a destructor
 * to run on the way through. `noinline` keeps them real frames. */
__attribute__((noinline)) int descend(int n)
{
    Guard g;
    if (n == 0)
        throw std::runtime_error("deep");
    return descend(n - 1) + 1;
}

__attribute__((noinline)) void throw_int(int v)
{
    throw v;
}

struct Base {
    virtual ~Base() = default;
    int code = 5;
};
struct Derived : Base {
    Derived() { code = 6; }
};

__attribute__((noinline)) void throw_derived()
{
    throw Derived();
}

} // namespace

int main()
{
    /* 1. dl_iterate_phdr reports the program -- the one object -- with a
     *    segment holding this code and the unwind index. */
    Found f;
    if (dl_iterate_phdr(record, &f) != 7)
        return 1;
    if (f.calls != 1)
        return 2;
    if (!f.named_empty)
        return 3;
    if (!f.holds_main)
        return 4;
    if (!f.has_eh_frame_hdr)
        return 5;

    /* 2. _dl_find_object finds the same index for an address in the program,
     *    and nothing for one outside it. */
    {
        struct dl_find_object d;
        if (_dl_find_object(reinterpret_cast<void *>(&record), &d) != 0)
            return 10;
        if (reinterpret_cast<ElfW(Addr)>(d.dlfo_eh_frame) != f.eh_frame_hdr)
            return 11;
        int on_stack = 0;
        if (_dl_find_object(&on_stack, &d) != -1)
            return 12;
    }

    /* 3. dlopen(NULL) is the program; its symbols are not looked up; the
     *    message is read once; the handle closes. */
    {
        void *h = dlopen(nullptr, RTLD_NOW);
        if (h == nullptr)
            return 20;
        if (dlsym(h, "main") != nullptr)
            return 21;
        const char *m = dlerror();
        if (m == nullptr || strstr(m, "undefined symbol: main") == nullptr)
            return 22;
        if (dlerror() != nullptr)
            return 23;
        if (dlclose(h) != 0)
            return 24;
    }

    /* 4. An int, thrown and caught. */
    try {
        throw_int(42);
        return 30;
    } catch (int v) {
        if (v != 42)
            return 31;
    } catch (...) {
        return 32;
    }

    /* 5. A std::runtime_error through fifty frames, each destructor run on
     *    the way, caught by its base class. */
    try {
        descend(50);
        return 40;
    } catch (const std::exception &e) {
        if (strcmp(e.what(), "deep") != 0)
            return 41;
        /* descend(50) down to descend(0): fifty-one frames, a Guard each. */
        if (destroyed != 51)
            return 43;
    }

    /* 6. Rethrown from a catch-all, caught outside. */
    try {
        try {
            throw_int(7);
        } catch (...) {
            throw;
        }
        return 50;
    } catch (int v) {
        if (v != 7)
            return 51;
    }

    /* 7. Kept as an exception_ptr and thrown again after its handler has
     *    finished. */
    {
        std::exception_ptr kept;
        try {
            throw std::logic_error("kept");
        } catch (...) {
            kept = std::current_exception();
        }
        if (!kept)
            return 60;
        try {
            std::rethrow_exception(kept);
        } catch (const std::logic_error &e) {
            if (strcmp(e.what(), "kept") != 0)
                return 61;
        } catch (...) {
            return 62;
        }
    }

    /* 8. A derived class caught as its base: the type matching RTTI does. */
    try {
        throw_derived();
        return 70;
    } catch (const Base &b) {
        if (b.code != 6)
            return 71;
    }

    return 42;
}

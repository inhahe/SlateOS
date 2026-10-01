"""glibc 2.39's <dlfcn.h>, in a statically linked program -- what every
program here is -- as the oracle for posix/src/dlfcn.rs.

    python posix/tools/oracle/dlfcn_harness.py   # writes posix/src/dlfcn_oracle.txt

One line a probe, `<probe> = <answer> | <message>`: the call's answer
(`handle` for the program's handle, `NULL`, or the numbers it returns and
writes) and what `dlerror` then says, `-` for nothing. The program's own name
in a message is written `<prog>`, since the tests' is another.

Probes whose answers here differ from glibc's by design are not replayed but
listed at the end as comments, with what they said:

- `dlopen` of a file: glibc's static `dlopen` can load a shared object, and
  says why it could not ("No such file or directory"); nothing can here, and
  the message says that instead. With `RTLD_NOLOAD` glibc searches for the
  file, and reports one it cannot find; nothing is searched here, and a file
  not loaded is the answer asked for, so there is no message -- which is
  glibc's own answer for a file it finds and has not loaded.
- `RTLD_DI_SERINFOSIZE`: glibc searches its default directories; here there
  are none to search, so the answer is an empty path.
- `RTLD_DI_ORIGIN`: the directory of whichever program runs.

And what does not depend on a probe's text but on the program's addresses
-- `dl_iterate_phdr`'s call, `_dl_find_object`'s segment, `dladdr` -- is
recorded as comments too, as the relations the tests check with a synthetic
image. Each `dl*` call is made on its own, then `dlerror` read: never two in
one argument list, whose evaluation order C leaves open.
"""

import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
from _wsl import POSIX_SRC, run, workdir, wsl_path  # noqa: E402

OUT = POSIX_SRC / "dlfcn_oracle.txt"

PROGRAM = r'''
#define _GNU_SOURCE
#include <dlfcn.h>
#include <elf.h>
#include <errno.h>
#include <link.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>

extern const ElfW(Ehdr) __ehdr_start;
__thread int tvar = 42;
static const char *prog;

/* The message, the program's name written <prog>. */
static void msg(void)
{
    const char *m = dlerror();
    if (!m) {
        printf("-\n");
        return;
    }
    size_t n = strlen(prog);
    if (strncmp(m, prog, n) == 0 && m[n] == ':')
        printf("<prog>%s\n", m + n);
    else
        printf("%s\n", m);
}

static void *H;

static void ptr(const char *probe, void *p)
{
    printf("%s = %s | ", probe, !p ? "NULL" : p == H ? "handle" : "other");
    msg();
}

static void num(const char *probe, long r)
{
    printf("%s = %ld | ", probe, r);
    msg();
}

#define P(probe, expr) do { dlerror(); void *r_ = (void *)(uintptr_t)(expr); ptr(probe, r_); } while (0)
#define N(probe, expr) do { dlerror(); long r_ = (long)(expr); num(probe, r_); } while (0)

static int calls;
static int cb(struct dl_phdr_info *info, size_t size, void *data)
{
    calls++;
    if (calls == 1)
        printf("# iterate: size=%zu name=\"%s\" addr=%#lx phdr_is_ehdr_plus_phoff=%d phnum_is_e_phnum=%d adds=%llu "
               "subs=%llu modid=%zu tls_data_holds_tvar_at=%ld\n",
               size, info->dlpi_name, (unsigned long)info->dlpi_addr,
               (const char *)info->dlpi_phdr == (const char *)&__ehdr_start + __ehdr_start.e_phoff,
               info->dlpi_phnum == __ehdr_start.e_phnum, info->dlpi_adds, info->dlpi_subs, info->dlpi_tls_modid,
               (long)((char *)&tvar - (char *)info->dlpi_tls_data));
    else
        printf("# iterate: then \"%s\" (the vDSO)\n", info->dlpi_name);
    return *(int *)data;
}

int main(int argc, char **argv)
{
    (void)argc;
    prog = strrchr(argv[0], '/') ? strrchr(argv[0], '/') + 1 : argv[0];
    setvbuf(stdout, NULL, _IONBF, 0);
    H = dlopen(NULL, RTLD_NOW);
    dlerror();
    const char *file = "libnonexistent.so";

    P("dlopen(NULL,RTLD_NOW)", dlopen(NULL, RTLD_NOW));
    P("dlopen(NULL,RTLD_LAZY)", dlopen(NULL, RTLD_LAZY));
    P("dlopen(NULL,0)", dlopen(NULL, 0));
    P("dlopen(NULL,0x1234)", dlopen(NULL, 0x1234));
    P("dlopen(NULL,RTLD_NOLOAD|RTLD_NOW)", dlopen(NULL, RTLD_NOLOAD | RTLD_NOW));
    P("dlopen(NULL,RTLD_NOW|RTLD_GLOBAL|RTLD_NODELETE|RTLD_DEEPBIND)",
      dlopen(NULL, RTLD_NOW | RTLD_GLOBAL | RTLD_NODELETE | RTLD_DEEPBIND));
    P("dlopen(\"\",RTLD_NOW)", dlopen("", RTLD_NOW));
    P("dlopen(file,0)", dlopen(file, 0));
    P("dlsym(program,x)", dlsym(H, "x"));
    P("dlsym(RTLD_DEFAULT,x)", dlsym(RTLD_DEFAULT, "x"));
    P("dlsym(RTLD_NEXT,x)", dlsym(RTLD_NEXT, "x"));
    P("dlvsym(program,x,GLIBC_2.2.5)", dlvsym(H, "x", "GLIBC_2.2.5"));
    P("dlvsym(RTLD_DEFAULT,x,GLIBC_2.2.5)", dlvsym(RTLD_DEFAULT, "x", "GLIBC_2.2.5"));
    N("dlclose(program)", dlclose(H));
    {
        Lmid_t id = 99;
        dlerror();
        long r = dlinfo(H, RTLD_DI_LMID, &id);
        printf("dlinfo(program,RTLD_DI_LMID) = %ld %ld | ", r, (long)id);
        msg();
    }
    {
        void *p = NULL;
        N("dlinfo(program,RTLD_DI_CONFIGADDR)", dlinfo(H, RTLD_DI_CONFIGADDR, &p));
        N("dlinfo(program,RTLD_DI_PROFILENAME)", dlinfo(H, RTLD_DI_PROFILENAME, &p));
        N("dlinfo(program,RTLD_DI_PROFILEOUT)", dlinfo(H, RTLD_DI_PROFILEOUT, &p));
        N("dlinfo(program,999)", dlinfo(H, 999, &p));
    }
    P("dlmopen(LM_ID_BASE,NULL,RTLD_NOW)", dlmopen(LM_ID_BASE, NULL, RTLD_NOW));
    P("dlmopen(LM_ID_NEWLM,NULL,RTLD_NOW)", dlmopen(LM_ID_NEWLM, NULL, RTLD_NOW));
    P("dlmopen(5,NULL,RTLD_NOW)", dlmopen(5, NULL, RTLD_NOW));
    P("dlmopen(LM_ID_NEWLM,file,RTLD_NOW)", dlmopen(LM_ID_NEWLM, file, RTLD_NOW));
    P("dlmopen(LM_ID_BASE,NULL,0x1234|RTLD_NOW)", dlmopen(LM_ID_BASE, NULL, 0x1234 | RTLD_NOW));

    /* Not replayed: see the docstring. */
    dlerror();
    dlopen(file, RTLD_NOW);
    printf("# dlopen(file,RTLD_NOW): ");
    msg();
    dlerror();
    dlopen(file, RTLD_NOLOAD | RTLD_NOW);
    printf("# dlopen(file,RTLD_NOLOAD|RTLD_NOW): ");
    msg();
    {
        Dl_serinfo si;
        dlerror();
        long r = dlinfo(H, RTLD_DI_SERINFOSIZE, &si);
        printf("# dlinfo(program,RTLD_DI_SERINFOSIZE) = %ld size=%zu cnt=%u (glibc's default directories)\n", r,
               si.dls_size, si.dls_cnt);
    }
    {
        struct link_map *lm = NULL;
        dlinfo(H, RTLD_DI_LINKMAP, &lm);
        printf("# dlinfo(program,RTLD_DI_LINKMAP): the handle=%d l_addr=%#lx l_name=\"%s\" l_ld=%s l_prev=%s\n",
               (void *)lm == H, (unsigned long)lm->l_addr, lm->l_name, lm->l_ld ? "set" : "NULL",
               lm->l_prev ? "set" : "NULL");
        size_t modid = 99;
        dlinfo(H, RTLD_DI_TLS_MODID, &modid);
        void *td = NULL;
        dlinfo(H, RTLD_DI_TLS_DATA, &td);
        const void *ph = NULL;
        long n = dlinfo(H, RTLD_DI_PHDR, &ph);
        printf("# dlinfo(program,TLS_MODID)=%zu TLS_DATA_holds_tvar_at=%ld PHDR=%ld is_e_phnum=%d at_ehdr_plus_phoff=%d\n",
               modid, (long)((char *)&tvar - (char *)td), n, n == __ehdr_start.e_phnum,
               (const char *)ph == (const char *)&__ehdr_start + __ehdr_start.e_phoff);
    }
    {
        int stop = 1, go = 0;
        calls = 0;
        int r = dl_iterate_phdr(cb, &stop);
        printf("# iterate: a callback answering 1 is called %d time(s), and the call answers %d\n", calls, r);
        calls = 0;
        r = dl_iterate_phdr(cb, &go);
        printf("# iterate: one answering 0 is called %d time(s), and the call answers %d\n", calls, r);
    }
    {
        Dl_info di;
        printf("# dladdr(main) = %d (a static program's map has no address range)\n", dladdr((void *)main, &di));
    }
    {
        struct dl_find_object r;
        int ret = _dl_find_object((void *)main, &r);
        const ElfW(Phdr) *ph = (const void *)((const char *)&__ehdr_start + __ehdr_start.e_phoff);
        int seg = -1;
        for (int i = 0; i < __ehdr_start.e_phnum; i++)
            if (ph[i].p_type == PT_LOAD && (char *)main >= (char *)ph[i].p_vaddr &&
                (char *)main < (char *)(ph[i].p_vaddr + ph[i].p_memsz))
                seg = i;
        printf("# _dl_find_object(main) = %d: the segment holding it=%d the link map=%d eh_frame=%s (linked without "
               ".eh_frame_hdr=%d)\n",
               ret,
               seg >= 0 && r.dlfo_map_start == (void *)ph[seg].p_vaddr &&
                   r.dlfo_map_end == (void *)(ph[seg].p_vaddr + ph[seg].p_memsz),
               (void *)r.dlfo_link_map == H, r.dlfo_eh_frame ? "set" : "NULL", 1);
        int argc_local = 0;
        printf("# _dl_find_object(stack) = %d\n", _dl_find_object(&argc_local, &r));
    }
    return 0;
}
'''


def main() -> None:
    with workdir() as t:
        d = Path(t)
        (d / "dl.c").write_text(PROGRAM, encoding="utf-8", newline="\n")
        r = run(f"cd {wsl_path(d)} && gcc -O0 -w -static -no-pie -o dl dl.c 2>/dev/null && ./dl")
        if r.returncode != 0:
            sys.exit(f"the harness failed:\n{r.stderr}\n{r.stdout[-2000:]}")
        body = r.stdout
    head = ("# glibc 2.39's <dlfcn.h> in a statically linked program, for posix/src/dlfcn.rs.\n"
            "# Generated by posix/tools/oracle/dlfcn_harness.py; do not edit.\n")
    OUT.write_text(head + body, encoding="utf-8", newline="\n")
    print(f"{OUT.name}: {body.count(chr(10))} lines")


if __name__ == "__main__":
    main()

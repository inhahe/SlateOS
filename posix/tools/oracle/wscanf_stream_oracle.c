/* fwscanf through a stream, glibc 2.39, C.UTF-8: what is read, what is left
 * for the next call, and the errors.  Each case prints one line; the tests in
 * posix/src/scanf.rs that cite this file by case name assert what it printed:
 *
 *   gcc -O1 -o wscanf_stream_oracle wscanf_stream_oracle.c
 *   LC_ALL=C.UTF-8 ./wscanf_stream_oracle
 *
 * which under glibc 2.39 (WSL, Ubuntu 24.04) prints
 *
 *   through ret=3 errno=0 a=12 w=e9.20ac.0 b=34 next=a
 *   giveback ret=1 errno=0 a=5 next=20ac
 *   bytestream ret=-1 errno=0 first=1 a=0 wide=-1
 *   badutf8_mid ret=1 errno=EILSEQ s=616200 ferror=1
 *   badutf8_first ret=-1 errno=EILSEQ a=7 ferror=1
 *   mismatch ret=0 errno=0 a=7 next=78
 *   writeonly ret=-1 errno=EBADF ferror=0
 *   byte_writeonly ret=-1 errno=EBADF ferror=0 wide=-1
 */
#define _GNU_SOURCE
#include <errno.h>
#include <locale.h>
#include <stdio.h>
#include <string.h>
#include <wchar.h>

static const char *ename(int e)
{
    return e == 0 ? "0" : e == EILSEQ ? "EILSEQ" : e == EBADF ? "EBADF" : e == EINVAL ? "EINVAL" : "other";
}

static FILE *mem(const char *s, const char *mode)
{
    /* A real file, written through one stream and read through a fresh one:
     * glibc's fmemopen streams are fopencookie streams, which have no wide
     * state (wide I/O on one faults), and a stream written with fputs is
     * byte-oriented for good. */
    static int n;
    char path[64];
    snprintf(path, sizeof path, "/tmp/wscanf_oracle_%d", n++);
    FILE *w = fopen(path, "w");
    fputs(s, w);
    fclose(w);
    return fopen(path, mode);
}

int main(void)
{
    setvbuf(stdout, NULL, _IONBF, 0);
    if (!setlocale(LC_ALL, "C.UTF-8"))
        return 1;
    {
        FILE *f = mem("12 \xc3\xa9\xe2\x82\xac 34\nrest", "r");
        int a = 0, b = 0;
        wchar_t w[8] = {0};
        errno = 0;
        int r = fwscanf(f, L"%d %ls %d", &a, w, &b);
        wint_t next = fgetwc(f);
        printf("through ret=%d errno=%s a=%d w=%x.%x.%x b=%d next=%x\n", r, ename(errno), a,
               (unsigned)w[0], (unsigned)w[1], (unsigned)w[2], b, (unsigned)next);
        fclose(f);
    }
    {
        FILE *f = mem("5\xe2\x82\xac", "r");
        int a = 0;
        errno = 0;
        int r = fwscanf(f, L"%d", &a);
        wint_t next = fgetwc(f);
        printf("giveback ret=%d errno=%s a=%d next=%x\n", r, ename(errno), a, (unsigned)next);
        fclose(f);
    }
    {
        FILE *f = mem("12", "r");
        int c = fgetc(f); /* byte orientation */
        int a = 0;
        errno = 0;
        int r = fwscanf(f, L"%d", &a);
        printf("bytestream ret=%d errno=%s first=%c a=%d wide=%d\n", r, ename(errno), c, a, fwide(f, 0));
        fclose(f);
    }
    {
        FILE *f = mem("ab\xff" "cd", "r");
        char s[16];
        memset(s, 0x55, sizeof s);
        errno = 0;
        int r = fwscanf(f, L"%s", s);
        printf("badutf8_mid ret=%d errno=%s s=%02x%02x%02x ferror=%d\n", r, ename(errno),
               (unsigned char)s[0], (unsigned char)s[1], (unsigned char)s[2], ferror(f) != 0);
        fclose(f);
    }
    {
        FILE *f = mem("\xff", "r");
        int a = 7;
        errno = 0;
        int r = fwscanf(f, L"%d", &a);
        printf("badutf8_first ret=%d errno=%s a=%d ferror=%d\n", r, ename(errno), a, ferror(f) != 0);
        fclose(f);
    }
    {
        FILE *f = mem("x", "r");
        int a = 7;
        errno = 0;
        int r = fwscanf(f, L"%d", &a);
        wint_t next = fgetwc(f);
        printf("mismatch ret=%d errno=%s a=%d next=%x\n", r, ename(errno), a, (unsigned)next);
        fclose(f);
    }
    {
        FILE *f = mem("", "w");
        int a = 7;
        errno = 0;
        int r = fwscanf(f, L"%d", &a);
        printf("writeonly ret=%d errno=%s ferror=%d\n", r, ename(errno), ferror(f) != 0);
        fclose(f);
    }
    {
        FILE *f = mem("", "w");
        int a = 7;
        errno = 0;
        int r = fscanf(f, "%d", &a);
        printf("byte_writeonly ret=%d errno=%s ferror=%d wide=%d\n", r, ename(errno), ferror(f) != 0, fwide(f, 0));
        fclose(f);
    }
    return 0;
}

"""glibc 2.39's IPv6 option and routing-header builders -- RFC 3542's
inet6_opt_* and inet6_rth_*, RFC 2292's inet6_option_* -- as the oracle for
posix/src/inet6.rs.

    python posix/tools/oracle/inet6_harness.py   # writes posix/src/inet6_oracle.txt

One line a probe, `<name> = <results> [<bytes in hex>]`: each call's return
value in order, then the buffer, where there is one. Offsets into the
buffer are written as numbers (`@N`), pointers as the offset they point at.
"""

import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
from _wsl import POSIX_SRC, run, workdir, wsl_path  # noqa: E402

OUT = POSIX_SRC / "inet6_oracle.txt"

PROGRAM = r'''
#define _GNU_SOURCE
#include <netinet/in.h>
#include <netinet/ip6.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <sys/socket.h>

static void hex(const void *p, size_t n)
{
    printf(" [");
    for (size_t i = 0; i < n; i++) printf("%02x", ((const unsigned char *)p)[i]);
    printf("]");
}

static void at(const void *base, const void *p)
{
    if (!p) printf(" NULL");
    else printf(" @%ld", (long)((const char *)p - (const char *)base));
}

int main(void)
{
    /* inet6_opt: the space, then a header built. */
    {
        printf("opt-space =");
        int off = inet6_opt_init(NULL, 0);
        printf(" %d", off);
        off = inet6_opt_append(NULL, 0, off, 0x10, 4, 4, NULL); printf(" %d", off);
        off = inet6_opt_append(NULL, 0, off, 0x11, 1, 1, NULL); printf(" %d", off);
        off = inet6_opt_append(NULL, 0, off, 0x12, 8, 8, NULL); printf(" %d", off);
        off = inet6_opt_finish(NULL, 0, off); printf(" %d\n", off);
    }
    {
        unsigned char buf[64];
        memset(buf, 0xee, sizeof buf);
        void *data;
        printf("opt-build =");
        int off = inet6_opt_init(buf, 32); printf(" %d", off);
        off = inet6_opt_append(buf, 32, off, 0x10, 4, 4, &data); printf(" %d", off); at(buf, data);
        uint32_t v32 = 0x11223344;
        printf(" %d", inet6_opt_set_val(data, 0, &v32, sizeof v32));
        off = inet6_opt_append(buf, 32, off, 0x11, 1, 1, &data); printf(" %d", off); at(buf, data);
        uint8_t v8 = 0x55;
        printf(" %d", inet6_opt_set_val(data, 0, &v8, 1));
        off = inet6_opt_append(buf, 32, off, 0x12, 8, 8, &data); printf(" %d", off); at(buf, data);
        uint64_t v64 = 0x0102030405060708ull;
        printf(" %d", inet6_opt_set_val(data, 0, &v64, 8));
        off = inet6_opt_finish(buf, 32, off); printf(" %d", off);
        hex(buf, 40);
        printf("\n");
        /* walk it back */
        printf("opt-next =");
        int o = 0; uint8_t type; socklen_t len;
        while ((o = inet6_opt_next(buf, 32, o, &type, &len, &data)) != -1) {
            printf(" %d:%02x:%u", o, type, (unsigned)len); at(buf, data);
        }
        printf(" end\n");
        printf("opt-find =");
        o = inet6_opt_find(buf, 32, 0, 0x12, &len, &data); printf(" %d:%u", o, (unsigned)len); at(buf, data);
        uint64_t back = 0;
        int got = inet6_opt_get_val(data, 0, &back, 8);
        printf(" %d %d", got, back == v64);
        o = inet6_opt_find(buf, 32, 0, 0x99, &len, &data); printf(" %d", o);
        printf("\n");
    }
    {
        /* the refusals */
        unsigned char buf[64];
        void *data;
        printf("opt-refused =");
        printf(" %d", inet6_opt_init(buf, 12));                    /* not a multiple of 8 */
        printf(" %d", inet6_opt_append(NULL, 0, 2, 0, 4, 4, NULL));  /* type 0: Pad1 */
        printf(" %d", inet6_opt_append(NULL, 0, 2, 1, 4, 4, NULL));  /* type 1: PadN */
        printf(" %d", inet6_opt_append(NULL, 0, 2, 5, 4, 3, NULL));  /* align 3 */
        printf(" %d", inet6_opt_append(NULL, 0, 2, 5, 2, 4, NULL));  /* align > len */
        printf(" %d", inet6_opt_append(NULL, 0, 2, 5, 256, 1, NULL)); /* len > 255 */
        printf(" %d", inet6_opt_append(NULL, 0, 1, 5, 4, 4, NULL));  /* offset < 2 */
        printf(" %d", inet6_opt_init(buf, 8));
        printf(" %d", inet6_opt_append(buf, 8, 2, 5, 8, 8, &data));  /* too long for 8 */
        printf(" %d", inet6_opt_finish(buf, 4, 6));                  /* extlen too small */
        printf(" %d", inet6_opt_append(NULL, 0, 2, 5, 0, 1, NULL));  /* len 0 */
        printf("\n");
    }
    {
        /* Padding of every size, before an 8-aligned option. */
        for (int pre = 0; pre <= 7; pre++) {
            unsigned char buf[64];
            memset(buf, 0xee, sizeof buf);
            void *data;
            int off = inet6_opt_init(buf, 48);
            if (pre) off = inet6_opt_append(buf, 48, off, 0x20, pre, 1, &data);
            off = inet6_opt_append(buf, 48, off, 0x21, 8, 8, &data);
            int end = inet6_opt_finish(buf, 48, off);
            printf("opt-pad %d = %d", pre, end);
            hex(buf, end > 0 ? end : 0);
            printf("\n");
        }
    }

    /* inet6_rth */
    {
        printf("rth-space = %u %u %u %u %u\n", (unsigned)inet6_rth_space(IPV6_RTHDR_TYPE_0, 0),
               (unsigned)inet6_rth_space(IPV6_RTHDR_TYPE_0, 3), (unsigned)inet6_rth_space(IPV6_RTHDR_TYPE_0, 127),
               (unsigned)inet6_rth_space(IPV6_RTHDR_TYPE_0, 128), (unsigned)inet6_rth_space(2, 1));
        unsigned char buf[128];
        memset(buf, 0xee, sizeof buf);
        void *r = inet6_rth_init(buf, sizeof buf, IPV6_RTHDR_TYPE_0, 3);
        printf("rth-init ="); at(buf, r);
        struct in6_addr a[4];
        for (int i = 0; i < 4; i++) { memset(&a[i], 0, 16); a[i].s6_addr[0] = 0x20; a[i].s6_addr[15] = i + 1; }
        for (int i = 0; i < 4; i++) printf(" %d", inet6_rth_add(buf, &a[i]));
        printf(" segs=%d", inet6_rth_segments(buf));
        for (int i = -1; i <= 3; i++) { struct in6_addr *g = inet6_rth_getaddr(buf, i); at(buf, g); }
        hex(buf, inet6_rth_space(IPV6_RTHDR_TYPE_0, 3));
        printf("\n");
        unsigned char out[128];
        memset(out, 0xee, sizeof out);
        printf("rth-reverse = %d", inet6_rth_reverse(buf, out));
        hex(out, inet6_rth_space(IPV6_RTHDR_TYPE_0, 3));
        printf(" %d", inet6_rth_reverse(buf, buf));
        hex(buf, inet6_rth_space(IPV6_RTHDR_TYPE_0, 3));
        printf("\n");
        printf("rth-refused =");
        at(buf, inet6_rth_init(buf, 20, IPV6_RTHDR_TYPE_0, 3));
        at(buf, inet6_rth_init(buf, sizeof buf, 2, 1));
        at(buf, inet6_rth_init(buf, sizeof buf, IPV6_RTHDR_TYPE_0, 128));
        printf("\n");
    }

    /* inet6_option (RFC 2292) */
    {
        printf("option-space = %d %d %d %d\n", inet6_option_space(0), inet6_option_space(1),
               inet6_option_space(6), inet6_option_space(20));
        union { struct cmsghdr c; unsigned char b[256]; } u;
        memset(u.b, 0xee, sizeof u.b);
        struct cmsghdr *cmsg;
        printf("option-init = %d", inet6_option_init(u.b, &cmsg, IPV6_HOPOPTS));
        at(u.b, cmsg);
        uint8_t opt1[] = {0x10, 4, 0xa1, 0xa2, 0xa3, 0xa4};
        printf(" %d", inet6_option_append(cmsg, opt1, 4, 2));
        uint8_t *p = inet6_option_alloc(cmsg, 3, 1, 0);
        at(u.b, p);
        if (p) { p[0] = 0x11; p[1] = 1; p[2] = 0xbb; }
        uint8_t opt3[] = {0x12, 8, 1, 2, 3, 4, 5, 6, 7, 8};
        printf(" %d", inet6_option_append(cmsg, opt3, 8, 0));
        printf(" len=%zu level=%d type=%d", (size_t)cmsg->cmsg_len, cmsg->cmsg_level, cmsg->cmsg_type);
        hex(u.b, cmsg->cmsg_len);
        printf("\n");
        printf("option-next =");
        uint8_t *t = NULL;
        while (inet6_option_next(cmsg, &t) == 0) { at(u.b, t); printf(":%02x", *t); }
        at(u.b, t);
        printf("\n");
        printf("option-find =");
        t = NULL;
        printf(" %d", inet6_option_find(cmsg, &t, 0x12)); at(u.b, t);
        t = NULL;
        printf(" %d", inet6_option_find(cmsg, &t, 0x99)); at(u.b, t);
        printf("\n");
        {
            /* A header built by hand: Pad1, an option, PadN of no data. */
            union { struct cmsghdr c; unsigned char b[64]; } h;
            memset(h.b, 0, sizeof h.b);
            struct cmsghdr *hc = &h.c;
            hc->cmsg_len = CMSG_LEN(8);
            hc->cmsg_level = IPPROTO_IPV6;
            hc->cmsg_type = IPV6_HOPOPTS;
            unsigned char ext[8] = {0x3b, 0x00, 0x00, 0x10, 0x01, 0xaa, 0x01, 0x00};
            memcpy(CMSG_DATA(hc), ext, 8);
            printf("option-next-pad1 =");
            uint8_t *q = NULL;
            int r;
            while ((r = inet6_option_next(hc, &q)) == 0) { at(h.b, q); printf(":%02x", *q); }
            printf(" %d", r); at(h.b, q);
            printf("\n");
        }
        printf("option-refused = %d", inet6_option_init(u.b, &cmsg, 7));
        printf(" %d", inet6_option_append(cmsg, opt1, 3, 0));
        printf(" %d", inet6_option_append(cmsg, opt1, 4, 5));
        at(u.b, inet6_option_alloc(cmsg, 3, 3, 0));
        printf(" hop=%d dst=%d\n", IPV6_HOPOPTS, IPV6_DSTOPTS);
    }
    return 0;
}
'''


def main() -> None:
    with workdir() as t:
        d = Path(t)
        (d / "i6.c").write_text(PROGRAM, encoding="utf-8", newline="\n")
        r = run(f"cd {wsl_path(d)} && gcc -O0 -w -o i6 i6.c && ./i6")
        if r.returncode != 0:
            sys.exit(f"the harness failed:\n{r.stderr}\n{r.stdout[-2000:]}")
        body = r.stdout
    head = ("# glibc 2.39's IPv6 option and routing-header builders, for posix/src/inet6.rs.\n"
            "# Generated by posix/tools/oracle/inet6_harness.py; do not edit.\n")
    OUT.write_text(head + body, encoding="utf-8", newline="\n")
    print(f"{OUT.name}: {body.count(chr(10))} lines")


if __name__ == "__main__":
    main()

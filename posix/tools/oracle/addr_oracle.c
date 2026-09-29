/* glibc's answers for the address-conversion functions, one line per case.
   stdin: lines "<kind> <hex>" -- kind s: a string (hex of its bytes),
   kind a6: 16 address bytes, kind a4: 4 address bytes,
   kind mk: "net host" as two hex u32s (inet_makeaddr/lnaof/netof use
   the first).  Output: one line per input. */
#define _GNU_SOURCE 1
#include <arpa/inet.h>
#include <errno.h>
#include <netinet/in.h>
#include <netinet/ether.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static int unhex(const char *h, unsigned char *out, size_t cap) {
    size_t n = 0;
    while (h[0] && h[1] && n < cap) {
        unsigned v;
        if (sscanf(h, "%2x", &v) != 1) break;
        out[n++] = (unsigned char)v;
        h += 2;
    }
    return (int)n;
}

static void hexout(const unsigned char *p, size_t n) {
    for (size_t i = 0; i < n; i++) printf("%02x", p[i]);
}

int main(void) {
    char line[4096];
    while (fgets(line, sizeof line, stdin)) {
        line[strcspn(line, "\r\n")] = 0;
        char *sp = strchr(line, ' ');
        const char *hex = sp ? sp + 1 : "";
        if (sp) *sp = 0;
        unsigned char buf[2048];
        int n = unhex(hex, buf, sizeof buf - 1);
        buf[n] = 0;
        if (!strcmp(line, "s")) {
            const char *s = (const char *)buf;
            struct in_addr a; memset(&a, 0xAA, sizeof a);
            int r = inet_aton(s, &a);
            printf("aton=%d:", r); hexout((unsigned char *)&a, 4);
            in_addr_t ia = inet_addr(s);
            printf(" addr="); hexout((unsigned char *)&ia, 4);
            unsigned char p4[4]; memset(p4, 0xAA, 4);
            errno = 0;
            r = inet_pton(AF_INET, s, p4);
            printf(" pton4=%d:", r); hexout(p4, 4);
            unsigned char p6[16]; memset(p6, 0xAA, 16);
            r = inet_pton(AF_INET6, s, p6);
            printf(" pton6=%d:", r); hexout(p6, 16);
            printf(" network=%08x", (unsigned)inet_network(s));
            struct ether_addr e; memset(&e, 0xAA, sizeof e);
            struct ether_addr *ep = ether_aton_r(s, &e);
            printf(" ether=%d:", ep != NULL); hexout(e.ether_addr_octet, 6);
            printf("\n");
        } else if (!strcmp(line, "a6") || !strcmp(line, "a4")) {
            int af = line[1] == '6' ? AF_INET6 : AF_INET;
            char out[64];
            printf("ntop=");
            const char *r = inet_ntop(af, buf, out, sizeof out);
            printf("%s", r ? r : "NULL");
            /* the smallest size that works */
            size_t need = r ? strlen(r) + 1 : 0;
            if (r) {
                errno = 0;
                const char *r2 = inet_ntop(af, buf, out, (socklen_t)(need - 1));
                printf(" short=%s/%d", r2 ? "ok" : "NULL", errno);
            }
            if (af == AF_INET) {
                struct in_addr ia; memcpy(&ia, buf, 4);
                printf(" ntoa=%s lnaof=%08x netof=%08x", inet_ntoa(ia),
                       (unsigned)inet_lnaof(ia), (unsigned)inet_netof(ia));
            }
            printf("\n");
        } else if (!strcmp(line, "mk")) {
            unsigned net = 0, host = 0;
            sscanf(hex, "%x %x", &net, &host);
            struct in_addr ia = inet_makeaddr(net, host);
            printf("makeaddr="); hexout((unsigned char *)&ia, 4); printf("\n");
        } else if (!strcmp(line, "el")) {
            struct ether_addr e; memset(&e, 0xAA, sizeof e);
            char host[2048]; memset(host, 0, sizeof host); strcpy(host, "-");
            int r = ether_line((const char *)buf, &e, host);
            printf("line=%d:", r); hexout(e.ether_addr_octet, 6);
            printf(":"); hexout((unsigned char *)host, strlen(host)); printf("\n");
        } else if (!strcmp(line, "e")) {
            struct ether_addr e; memcpy(e.ether_addr_octet, buf, 6);
            char out[32];
            printf("ntoa=%s\n", ether_ntoa_r(&e, out));
        } else {
            printf("?\n");
        }
    }
    return 0;
}

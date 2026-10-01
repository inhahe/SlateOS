/* getifaddrs oracle: print every entry glibc returns, field by field, so the
 * port's tests can replay the structure (order, flags, sockaddr_ll fields,
 * which pointers are NULL).  Statistics values are the host's and are not
 * printed -- only whether ifa_data is there.  AF_INET6 entries are printed
 * too; the harness drops them (this system has no IPv6). */
#define _GNU_SOURCE
#include <arpa/inet.h>
#include <ifaddrs.h>
#include <linux/if_link.h>
#include <net/if.h>
#include <netpacket/packet.h>
#include <stddef.h>
#include <stdio.h>
#include <string.h>
#include <sys/socket.h>

static void sa(const char *tag, const struct sockaddr *s)
{
    printf(" %s=", tag);
    if (!s) {
        printf("-");
        return;
    }
    if (s->sa_family == AF_INET) {
        const struct sockaddr_in *i = (const void *)s;
        char b[32];
        inet_ntop(AF_INET, &i->sin_addr, b, sizeof b);
        printf("in(%s,port=%u,zero=", b, ntohs(i->sin_port));
        for (int k = 0; k < 8; k++)
            printf("%02x", i->sin_zero[k]);
        printf(")");
    } else if (s->sa_family == AF_PACKET) {
        const struct sockaddr_ll *l = (const void *)s;
        printf("ll(proto=%u,ifindex=%d,hatype=%u,pkttype=%u,halen=%u,addr=",
               ntohs(l->sll_protocol), l->sll_ifindex, l->sll_hatype,
               l->sll_pkttype, l->sll_halen);
        for (int k = 0; k < 8; k++)
            printf("%s%02x", k ? ":" : "", l->sll_addr[k]);
        printf(")");
    } else {
        printf("family%u", s->sa_family);
    }
}

int main(void)
{
    printf("layout sockaddr_ll=%zu family@%zu protocol@%zu ifindex@%zu hatype@%zu "
           "pkttype@%zu halen@%zu addr@%zu rtnl_link_stats=%zu\n",
           sizeof(struct sockaddr_ll), offsetof(struct sockaddr_ll, sll_family),
           offsetof(struct sockaddr_ll, sll_protocol),
           offsetof(struct sockaddr_ll, sll_ifindex),
           offsetof(struct sockaddr_ll, sll_hatype),
           offsetof(struct sockaddr_ll, sll_pkttype),
           offsetof(struct sockaddr_ll, sll_halen),
           offsetof(struct sockaddr_ll, sll_addr),
           sizeof(struct rtnl_link_stats));
    struct ifaddrs *list;
    if (getifaddrs(&list) != 0) {
        perror("getifaddrs");
        return 1;
    }
    for (struct ifaddrs *a = list; a; a = a->ifa_next) {
        printf("entry name=%s family=%u flags=%#x", a->ifa_name,
               a->ifa_addr ? a->ifa_addr->sa_family : 0, a->ifa_flags);
        sa("addr", a->ifa_addr);
        sa("netmask", a->ifa_netmask);
        sa("broadaddr", a->ifa_broadaddr);
        printf(" data=%s\n", a->ifa_data ? "stats" : "-");
    }
    freeifaddrs(list);
    return 0;
}

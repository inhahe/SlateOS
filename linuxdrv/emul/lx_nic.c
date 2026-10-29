// SPDX-License-Identifier: MIT
/*
 * A network driver process: what Linux's network core needs beneath it,
 * and the task that uses the devices the driver registers.
 *
 * The network core is Linux's own (net/core, net/ethernet, net/sched; the
 * driver's manifest lists them): devices registered, opened and queued to,
 * NAPI polling, socket buffers, the default queueing discipline. What is
 * not compiled in is everything above a device -- sockets, protocols,
 * netlink -- which a driver process has no use for: there is no user of the
 * device in the process but the layer's own task here. So:
 *
 * - Start-up: what Linux's start_kernel() and socket layer do for the core
 *   -- the initial network namespace set up (net_ns_init) and the socket
 *   buffer caches made (skb_init) -- done at the first initcall level,
 *   before the core's own initcalls.
 * - The RTNL, the network core's one big lock, is a mutex here as in Linux;
 *   rtnl_unlock() runs the deferred work of unregistering devices, as
 *   Linux's does. Netlink's notifications go to no one: there are no
 *   listeners.
 * - The task (lx-nic): opens every device the driver registered and takes
 *   what it receives (an rx handler, so nothing above the device is asked).
 *   For now it does one thing, the "check" the boot test runs: once the
 *   link is up, it asks for QEMU's user-network gateway (10.0.2.2) from
 *   10.0.2.15, then pings it 400 times with 1000 bytes each, every answer's
 *   payload checked -- frames sent and received through the driver, its
 *   interrupts, its DMA and its receive ring's page fragments -- and ends
 *   the process with the verdict. Serving the native network stack comes
 *   next.
 */
#include <linux/netdevice.h>
#include <linux/etherdevice.h>
#include <linux/if_arp.h>
#include <linux/rtnetlink.h>
#include <linux/kthread.h>
#include <linux/skbuff.h>
#include <linux/completion.h>
#include <linux/delay.h>
#include <linux/init.h>
#include <linux/ethtool.h>
#include <linux/filter.h>
#include <linux/bpf.h>
#include <linux/pipe_fs_i.h>
#include <linux/stringhash.h>
#include <linux/random.h>
#include <linux/wait.h>
#include <linux/ip.h>
#include <linux/icmp.h>
#include <net/checksum.h>
#include <net/net_namespace.h>
#include <net/sock.h>

#include "slx.h"
#include "lx_emul.h"

/* What start_kernel() and the socket layer set up for the network core. */
static int __init lx_net_init(void)
{
	net_ns_init();
	skb_init();
	return 0;
}
pure_initcall(lx_net_init);

static DEFINE_MUTEX(lx_rtnl_mutex);

void rtnl_lock(void)
{
	mutex_lock(&lx_rtnl_mutex);
}

int rtnl_lock_killable(void)
{
	return mutex_lock_killable(&lx_rtnl_mutex);
}

int rtnl_trylock(void)
{
	return mutex_trylock(&lx_rtnl_mutex);
}

int rtnl_is_locked(void)
{
	return mutex_is_locked(&lx_rtnl_mutex);
}

void __rtnl_unlock(void)
{
	mutex_unlock(&lx_rtnl_mutex);
}

/* net/core/dev.c's, declared in the core's own header (net/core/dev.h), not include/. */
void netdev_run_todo(void);

/* As Linux's: the devices waiting to be unregistered are, and it lets go. */
void rtnl_unlock(void)
{
	netdev_run_todo();
}

/* Netlink's handlers for the core's messages: with no netlink, none are kept. */
int __rtnl_register_many(const struct rtnl_msg_handler *handlers, int n)
{
	return 0;
}

/* The core's files under /proc/net: a driver process has no procfs. */
int __init dev_proc_init(void);
int __init dev_proc_init(void)
{
	return 0;
}

/*
 * A name's hash, for the core's tables of devices by name (fs/namei.c's in
 * Linux, a file a driver process does not compile): Linux's own
 * incremental string hash, a byte at a time.
 */
unsigned int full_name_hash(const void *salt, const char *name, unsigned int len)
{
	unsigned long hash = init_name_hash(salt);

	while (len--)
		hash = partial_name_hash((unsigned char)*name++, hash);
	return end_name_hash(hash);
}

/*
 * The key a device spreads received flows over its queues by (RSS): one
 * per process, random, made when first asked for, as Linux's (net/ethtool,
 * not compiled in).
 */
void netdev_rss_key_fill(void *buffer, size_t len)
{
	static u8 key[NETDEV_RSS_KEY_LEN];
	static bool made;

	if (WARN_ON(len > sizeof(key)))
		len = sizeof(key);
	if (!made) {
		get_random_bytes(key, sizeof(key));
		made = true;
	}
	memcpy(buffer, key, len);
}

/* A device's change, told to netlink's listeners: a driver process has none. */
void rtmsg_ifinfo(int type, struct net_device *dev, unsigned int change, gfp_t flags,
		  u32 portid, const struct nlmsghdr *nlh)
{
}

/*
 * What the core refers to of the layers above it, which are not compiled
 * in: the socket buffer destructors it compares a buffer's against -- no
 * socket ever owns one here, so none is called -- and the static keys it
 * tests, all off.
 */
void sock_wfree(struct sk_buff *skb)
{
	BUG();
}

void __sock_wfree(struct sk_buff *skb)
{
	BUG();
}

void sock_efree(struct sk_buff *skb)
{
	BUG();
}

DEFINE_STATIC_KEY_FALSE(memalloc_socks_key);
DEFINE_STATIC_KEY_FALSE(bpf_master_redirect_enabled_key);
DEFINE_STATIC_KEY_FALSE(bpf_stats_enabled_key);

/* A socket buffer spliced into a pipe: never, with neither here. */
const struct pipe_buf_operations nosteal_pipe_buf_ops = {};

/*
 * ethtool's answers for drivers that have nothing better (net/ethtool's
 * ioctl interface, which is not compiled in, keeps Linux's): the link is up
 * when the carrier is, and time stamps are software's, no clock of the
 * device's own.
 */
u32 ethtool_op_get_link(struct net_device *dev)
{
	return netif_carrier_ok(dev) ? 1 : 0;
}

int ethtool_op_get_ts_info(struct net_device *dev, struct kernel_ethtool_ts_info *info)
{
	info->so_timestamping = SOF_TIMESTAMPING_TX_SOFTWARE | SOF_TIMESTAMPING_RX_SOFTWARE |
				SOF_TIMESTAMPING_SOFTWARE;
	info->phc_index = -1;
	return 0;
}

/*
 * The "check" the kernel's self-test runs. Our address and the gateway's
 * are QEMU's user network's defaults; its gateway answers an ARP request
 * for its own address, and echoes a ping to it, whoever asks.
 */
#define LX_NIC_OURS 0x0a00020f		/* 10.0.2.15 */
#define LX_NIC_GATEWAY 0x0a000202	/* 10.0.2.2 */

/*
 * How many pings, and how much each carries: enough full-size frames -- past
 * e1000e's copybreak, so each is received into a buffer of its ring -- for
 * the ring (256 buffers) to wrap onto the buffers its refills cut from page
 * fragments, and for whole fragment pages to be used up and given back.
 */
#define LX_NIC_PINGS 400
#define LX_NIC_PING_BYTES 1000

struct lx_nic_arp {
	__be16 htype, ptype;
	u8 hlen, plen;
	__be16 op;
	u8 sha[ETH_ALEN];
	__be32 spa;
	u8 tha[ETH_ALEN];
	__be32 tpa;
} __packed;

struct lx_nic_icmp {
	u8 type, code;
	__sum16 checksum;
	__be16 id, seq;
} __packed;

static DECLARE_COMPLETION(lx_nic_answered);
static u8 lx_nic_gateway_mac[ETH_ALEN];
static unsigned long lx_nic_received;
/* The ping last answered, its payload as sent or not; woken each answer. */
static DECLARE_WAIT_QUEUE_HEAD(lx_nic_echoed);
static int lx_nic_echo_seq = -1;
static unsigned long lx_nic_echo_bad;

/* Byte `i` of ping `seq`'s payload. */
static u8 lx_nic_payload(unsigned int seq, unsigned int i)
{
	return (u8)(seq * 7 + i);
}

/* An echo reply from the gateway to us: which ping it answers, its payload checked. */
static void lx_nic_echo(struct sk_buff *skb)
{
	const struct iphdr *ip = (const struct iphdr *)skb->data;
	const struct lx_nic_icmp *icmp;
	const u8 *payload;
	unsigned int seq, i;

	if (!pskb_may_pull(skb, sizeof(*ip) + sizeof(*icmp) + LX_NIC_PING_BYTES))
		return;
	ip = (const struct iphdr *)skb->data;
	if (ip->ihl != 5 || ip->protocol != IPPROTO_ICMP || ip->saddr != htonl(LX_NIC_GATEWAY) ||
	    ip->daddr != htonl(LX_NIC_OURS))
		return;
	icmp = (const struct lx_nic_icmp *)(ip + 1);
	if (icmp->type != ICMP_ECHOREPLY || icmp->id != htons(0x5157))
		return;
	seq = ntohs(icmp->seq);
	payload = (const u8 *)(icmp + 1);
	for (i = 0; i < LX_NIC_PING_BYTES; i++)
		if (payload[i] != lx_nic_payload(seq, i)) {
			lx_nic_echo_bad++;
			break;
		}
	WRITE_ONCE(lx_nic_echo_seq, seq);
	wake_up_all(&lx_nic_echoed);
}

/*
 * Every frame the device receives: counted, the gateway's ARP answer and
 * echo replies noted, all consumed.
 */
static rx_handler_result_t lx_nic_rx(struct sk_buff **pskb)
{
	struct sk_buff *skb = *pskb;
	const struct lx_nic_arp *arp;

	lx_nic_received++;
	if (skb->protocol == htons(ETH_P_ARP) && pskb_may_pull(skb, sizeof(*arp))) {
		arp = (const struct lx_nic_arp *)skb->data;
		if (arp->op == htons(ARPOP_REPLY) && arp->spa == htonl(LX_NIC_GATEWAY) &&
		    arp->tpa == htonl(LX_NIC_OURS) && !completion_done(&lx_nic_answered)) {
			ether_addr_copy(lx_nic_gateway_mac, arp->sha);
			complete(&lx_nic_answered);
		}
	} else if (skb->protocol == htons(ETH_P_IP)) {
		lx_nic_echo(skb);
	}
	kfree_skb(skb);
	return RX_HANDLER_CONSUMED;
}

/* A frame of `bytes` after its Ethernet header to `dest`, of type `proto`, its body to fill. */
static struct sk_buff *lx_nic_frame(struct net_device *dev, const u8 *dest, u16 proto,
				    unsigned int bytes)
{
	struct sk_buff *skb = alloc_skb(ETH_HLEN + max_t(unsigned int, bytes, ETH_ZLEN), GFP_KERNEL);
	struct ethhdr *eth;

	if (!skb)
		return NULL;
	eth = skb_put(skb, ETH_HLEN);
	ether_addr_copy(eth->h_dest, dest);
	ether_addr_copy(eth->h_source, dev->dev_addr);
	eth->h_proto = htons(proto);
	skb_reset_mac_header(skb);
	skb->dev = dev;
	skb->protocol = htons(proto);
	return skb;
}

/* An ARP request for the gateway, queued to `dev`: 0, or why not. */
static int lx_nic_ask(struct net_device *dev)
{
	struct sk_buff *skb = lx_nic_frame(dev, dev->broadcast, ETH_P_ARP, sizeof(struct lx_nic_arp));
	struct lx_nic_arp *arp;

	if (!skb)
		return -ENOMEM;
	arp = skb_put_zero(skb, sizeof(*arp));
	arp->htype = htons(ARPHRD_ETHER);
	arp->ptype = htons(ETH_P_IP);
	arp->hlen = ETH_ALEN;
	arp->plen = 4;
	arp->op = htons(ARPOP_REQUEST);
	ether_addr_copy(arp->sha, dev->dev_addr);
	arp->spa = htonl(LX_NIC_OURS);
	arp->tpa = htonl(LX_NIC_GATEWAY);
	return dev_queue_xmit(skb);
}

/* Ping `seq` to the gateway, queued to `dev`: 0, or why not. */
static int lx_nic_ping(struct net_device *dev, unsigned int seq)
{
	unsigned int len = sizeof(struct lx_nic_icmp) + LX_NIC_PING_BYTES;
	struct sk_buff *skb = lx_nic_frame(dev, lx_nic_gateway_mac, ETH_P_IP,
					   sizeof(struct iphdr) + len);
	struct lx_nic_icmp *icmp;
	struct iphdr *ip;
	u8 *payload;
	unsigned int i;

	if (!skb)
		return -ENOMEM;
	ip = skb_put_zero(skb, sizeof(*ip));
	ip->version = 4;
	ip->ihl = 5;
	ip->tot_len = htons(sizeof(*ip) + len);
	ip->id = htons(seq);
	ip->ttl = 64;
	ip->protocol = IPPROTO_ICMP;
	ip->saddr = htonl(LX_NIC_OURS);
	ip->daddr = htonl(LX_NIC_GATEWAY);
	ip->check = ip_fast_csum(ip, ip->ihl);
	icmp = skb_put_zero(skb, sizeof(*icmp));
	icmp->type = ICMP_ECHO;
	icmp->id = htons(0x5157);
	icmp->seq = htons(seq);
	payload = skb_put(skb, LX_NIC_PING_BYTES);
	for (i = 0; i < LX_NIC_PING_BYTES; i++)
		payload[i] = lx_nic_payload(seq, i);
	icmp->checksum = csum_fold(csum_partial(icmp, len, 0));
	return dev_queue_xmit(skb);
}

/*
 * The check on `dev`, opened: the link up within 10 s; an ARP request a
 * second until the gateway answers, 10 at most -- the first may meet the
 * queue that drops everything until the link's news has been acted on
 * (link_watch); then LX_NIC_PINGS pings, one at a time, each answered within
 * a second -- sent again once if not -- with its payload as sent. 0, or why
 * not.
 */
static int lx_nic_check(struct net_device *dev)
{
	unsigned long deadline = jiffies + 10 * HZ;
	unsigned int seq, resent = 0;
	int tries, err;

	while (!netif_carrier_ok(dev) && time_before(jiffies, deadline))
		msleep(20);
	if (!netif_carrier_ok(dev)) {
		netdev_err(dev, "no link within 10 s\n");
		return -ENOLINK;
	}
	for (tries = 0; tries < 10; tries++) {
		err = lx_nic_ask(dev);
		if (err < 0) {
			netdev_err(dev, "an ARP request could not be queued: %d\n", err);
			return err;
		}
		if (wait_for_completion_timeout(&lx_nic_answered, HZ))
			break;
	}
	if (tries == 10) {
		netdev_err(dev, "no ARP answer from the gateway in 10 s (%lu frame(s) received)\n",
			   lx_nic_received);
		return -ETIMEDOUT;
	}
	netdev_info(dev, "%pM: the gateway (10.0.2.2) is at %pM, after %d request(s)\n",
		    dev->dev_addr, lx_nic_gateway_mac, tries + 1);
	for (seq = 0; seq < LX_NIC_PINGS; seq++) {
		for (tries = 0; tries < 2; tries++) {
			err = lx_nic_ping(dev, seq);
			if (err < 0) {
				netdev_err(dev, "ping %u could not be queued: %d\n", seq, err);
				return err;
			}
			if (wait_event_timeout(lx_nic_echoed, READ_ONCE(lx_nic_echo_seq) == seq, HZ))
				break;
			resent++;
		}
		if (tries == 2) {
			netdev_err(dev, "ping %u unanswered (%u answered before it)\n", seq, seq);
			return -ETIMEDOUT;
		}
	}
	if (lx_nic_echo_bad) {
		netdev_err(dev, "%lu ping(s) answered with a payload other than sent\n", lx_nic_echo_bad);
		return -EIO;
	}
	netdev_info(dev, "%u pings of %u bytes answered, payloads as sent (%u sent again), %lu frame(s) received\n",
		    LX_NIC_PINGS, LX_NIC_PING_BYTES, resent, lx_nic_received);
	return 0;
}

static bool lx_nic_mode(const char *mode)
{
	int i;

	for (i = 1; i < slx_argc(); i++)
		if (!strcmp(slx_argv(i), mode))
			return true;
	return false;
}

/* The devices opened and listened to; then the check, which ends the process. */
static int lx_nic_task(void *unused)
{
	struct net_device *dev, *first = NULL;
	int err = 0;

	rtnl_lock();
	for_each_netdev(&init_net, dev) {
		if (dev->flags & IFF_LOOPBACK)
			continue;
		err = dev_open(dev, NULL);
		if (!err)
			err = netdev_rx_handler_register(dev, lx_nic_rx, NULL);
		if (err) {
			netdev_err(dev, "could not be opened and listened to: %d\n", err);
			break;
		}
		if (!first)
			first = dev;
	}
	rtnl_unlock();
	if (!err && !first) {
		pr_err("lx-nic: the driver registered no network device\n");
		err = -ENODEV;
	}
	if (!err && lx_nic_mode("check"))
		err = lx_nic_check(first);
	if (lx_nic_mode("check") || err)
		slx_exit(err ? SLX_EXIT_INIT_FAILED : SLX_EXIT_OK);
	return 0;
}

/* After the driver's own initcalls, which register its devices as they probe. */
static int __init lx_nic_start(void)
{
	struct task_struct *task = kthread_run(lx_nic_task, NULL, "lx-nic");

	return PTR_ERR_OR_ZERO(task);
}
late_initcall(lx_nic_start);

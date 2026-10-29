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
 * - The task (lx-nic), with "serve=<name>": the first device the driver
 *   registered opened and, once its link is up, served to the network
 *   stack as the service "nic.<name>" (design-decisions 1589; the kit's
 *   nicport.rs). Every frame it receives is copied into the next slot the
 *   stack posted, from an rx handler, so nothing above the device is asked;
 *   every frame the stack submits is sent by the task. The stack is told
 *   the device's carrier as its link, and the multicast addresses it lists
 *   become the device's own (dev_mc_add), until it lists others or leaves.
 *   With "once" as well,
 *   the process ends when the first stack to attach has gone -- the
 *   kernel's self-test, which a warning from Linux code anywhere in the
 *   process fails -- and without, the card waits for the next.
 */
#include <linux/netdevice.h>
#include <linux/etherdevice.h>
#include <linux/rtnetlink.h>
#include <linux/kthread.h>
#include <linux/skbuff.h>
#include <linux/delay.h>
#include <linux/init.h>
#include <linux/ethtool.h>
#include <linux/filter.h>
#include <linux/bpf.h>
#include <linux/pipe_fs_i.h>
#include <linux/stringhash.h>
#include <linux/random.h>
#include <linux/wait.h>
#include <linux/if_vlan.h>
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

/* What the card itself counted, sent and received, said beside the layer's own counts. */
static void lx_nic_card_counts(struct net_device *dev)
{
	struct rtnl_link_stats64 stats;

	dev_get_stats(dev, &stats);
	netdev_info(dev, "the card counted %llu frame(s) sent, %llu received\n",
		    stats.tx_packets, stats.rx_packets);
}

/* Whether `dev`'s link is up within 10 s. */
static bool lx_nic_link(struct net_device *dev)
{
	unsigned long deadline = jiffies + 10 * HZ;

	while (!netif_carrier_ok(dev) && time_before(jiffies, deadline))
		msleep(20);
	if (netif_carrier_ok(dev))
		return true;
	netdev_err(dev, "no link within 10 s\n");
	return false;
}

/*
 * The served device's frames, counted: received into the stack's slots,
 * dropped -- no slot posted, too long for the next, or its VLAN tag
 * unplaceable -- and sent. Woken at a turn that left the task work.
 */
static unsigned long lx_nic_port_received, lx_nic_port_dropped, lx_nic_port_sent;
static DECLARE_WAIT_QUEUE_HEAD(lx_nic_port_work);
/* The device served, from before the turn hook is set. */
static struct net_device *lx_nic_port_dev;

/*
 * The turn hook: the stack's connection seen to, the device's carrier its
 * link -- set and cleared by the driver as Linux's own watchdog sees the
 * card's -- and the task woken for it.
 */
static void lx_nic_port_turn(u64 now_ns)
{
	if (slx_nicport_turn(now_ns, netif_carrier_ok(lx_nic_port_dev)))
		wake_up(&lx_nic_port_work);
}

/*
 * What the stack's lists made the device pass: the addresses added
 * (dev_mc_add), which the next list is told apart from, and whether every
 * multicast frame.
 */
static u8 lx_nic_mc[SLX_NICPORT_MCAST_MAX][ETH_ALEN];
static long lx_nic_mc_n;
static bool lx_nic_allmulti;

/* Whether `addr` is among the `n` six-byte addresses at `list`. */
static bool lx_nic_listed(const u8 *addr, const u8 *list, long n)
{
	long i;

	for (i = 0; i < n; i++)
		if (ether_addr_equal(addr, list + ETH_ALEN * i))
			return true;
	return false;
}

/* Every address the stack's lists added dropped, and every-frame mode left. */
static void lx_nic_drop_mcast(struct net_device *dev)
{
	long i;

	for (i = 0; i < lx_nic_mc_n; i++)
		dev_mc_del(dev, lx_nic_mc[i]);
	lx_nic_mc_n = 0;
	if (lx_nic_allmulti) {
		/* Only an increment can fail: an overflow. */
		WARN_ON(dev_set_allmulti(dev, -1));
		lx_nic_allmulti = false;
	}
}

/*
 * The multicast addresses `dev` passes made the `n` six-byte addresses at
 * `list`, and every one with `all`, in place of the last: 0, or a negative
 * errno -- the device then passing none of them, as if none were listed.
 * Those leaving are taken out first, then those coming put in, each change
 * writing the card's filter (dev_mc_del/add): one in both lists never leaves
 * it.
 */
static int lx_nic_set_mcast(struct net_device *dev, const u8 *list, long n, bool all)
{
	u8 next[SLX_NICPORT_MCAST_MAX][ETH_ALEN];
	long i, kept = 0;
	int err = 0;

	rtnl_lock();
	for (i = 0; i < lx_nic_mc_n; i++) {
		if (lx_nic_listed(lx_nic_mc[i], list, n))
			memcpy(next[kept++], lx_nic_mc[i], ETH_ALEN);
		else
			dev_mc_del(dev, lx_nic_mc[i]);
	}
	for (i = 0; i < n && !err; i++) {
		const u8 *addr = list + ETH_ALEN * i;

		if (lx_nic_listed(addr, next[0], kept))
			continue;
		err = dev_mc_add(dev, addr);
		if (!err)
			memcpy(next[kept++], addr, ETH_ALEN);
	}
	memcpy(lx_nic_mc, next, sizeof(next[0]) * kept);
	lx_nic_mc_n = kept;
	if (!err && all != lx_nic_allmulti) {
		err = dev_set_allmulti(dev, all ? 1 : -1);
		if (!err)
			lx_nic_allmulti = all;
	}
	if (err)
		lx_nic_drop_mcast(dev);
	rtnl_unlock();
	return err;
}

/* The multicast list the stack sent, if any, put in force and answered. */
static void lx_nic_port_mcast(struct net_device *dev)
{
	static u8 list[6 * SLX_NICPORT_MCAST_MAX];
	bool all;
	long n = slx_nicport_mcast_next(list, &all);

	if (n >= 0)
		slx_nicport_mcast_done(lx_nic_set_mcast(dev, list, n, all));
}

/*
 * Every frame the served device receives, whole -- its Ethernet header, and
 * its VLAN tag where the card took it out -- copied into the next slot the
 * stack posted, or dropped.
 */
static rx_handler_result_t lx_nic_port_rx(struct sk_buff **pskb)
{
	struct sk_buff *skb = *pskb;
	void *slot;
	long room;

	skb_push(skb, skb->data - skb_mac_header(skb));
	if (skb_vlan_tag_present(skb)) {
		/* Frees the frame when it cannot. */
		skb = __vlan_hwaccel_push_inside(skb);
		if (!skb) {
			lx_nic_port_dropped++;
			return RX_HANDLER_CONSUMED;
		}
	}
	room = slx_nicport_rx_slot(&slot);
	if (room <= 0) {
		lx_nic_port_dropped++;
	} else if (skb->len > room || skb_copy_bits(skb, 0, slot, skb->len)) {
		slx_nicport_rx_done(-1);
		lx_nic_port_dropped++;
	} else {
		slx_nicport_rx_done(skb->len);
		lx_nic_port_received++;
	}
	consume_skb(skb);
	return RX_HANDLER_CONSUMED;
}

/* A frame the stack submitted, `len` bytes at `frame`, sent on `dev`: `len`, or a negative errno. */
static long lx_nic_port_send(struct net_device *dev, const void *frame, long len)
{
	struct sk_buff *skb = alloc_skb(len, GFP_KERNEL);
	int err;

	if (!skb)
		return -ENOMEM;
	skb_put_data(skb, frame, len);
	skb_reset_mac_header(skb);
	skb_set_network_header(skb, ETH_HLEN);
	skb->dev = dev;
	skb->protocol = eth_hdr(skb)->h_proto;
	err = dev_queue_xmit(skb);
	/*
	 * Congestion still queued it; any other answer dropped it.
	 * net_xmit_eval() is a macro that evaluates its argument twice.
	 */
	err = net_xmit_eval(err);
	if (err > 0)
		err = -ENOBUFS;
	if (err)
		return err;
	lx_nic_port_sent++;
	return len;
}

/*
 * `dev`, its link up, served as "nic.<name>": what the stack submits sent,
 * until -- with "once" -- the first stack has gone. 0, or why not.
 */
static int lx_nic_serve(struct net_device *dev, const char *name, bool once)
{
	const void *frame;
	long len, err;

	if (!lx_nic_link(dev))
		return -ENOLINK;
	err = slx_nicport_open(name, dev->dev_addr, dev->mtu);
	if (err) {
		netdev_err(dev, "could not be served as nic.%s: %ld\n", name, err);
		return -EPERM;
	}
	lx_nic_port_dev = dev;
	lx_emul_turn_hook = lx_nic_port_turn;
	netdev_info(dev, "%pM served to the network stack as nic.%s\n", dev->dev_addr, name);
	for (;;) {
		wait_event(lx_nic_port_work, slx_nicport_work());
		if (!slx_nicport_serving()) {
			netdev_err(dev, "nic.%s is no longer served\n", name);
			return -EPIPE;
		}
		if (slx_nicport_ended()) {
			netdev_info(dev, "a network stack left: %lu frame(s) sent for it, %lu received, %lu dropped\n",
				    lx_nic_port_sent, lx_nic_port_received, lx_nic_port_dropped);
			lx_nic_card_counts(dev);
			/*
			 * Its groups go with it, before the next stack is
			 * accepted -- the kit waits for this -- which starts
			 * from none.
			 */
			rtnl_lock();
			lx_nic_drop_mcast(dev);
			rtnl_unlock();
			if (once)
				return 0;
		}
		lx_nic_port_mcast(dev);
		while ((len = slx_nicport_tx_next(&frame)) > 0)
			slx_nicport_tx_done(lx_nic_port_send(dev, frame, len));
	}
}

static bool lx_nic_mode(const char *mode)
{
	int i;

	for (i = 1; i < slx_argc(); i++)
		if (!strcmp(slx_argv(i), mode))
			return true;
	return false;
}

/* The argument "<key><value>": its value, or NULL. */
static const char *lx_nic_arg(const char *key)
{
	size_t n = strlen(key);
	int i;

	for (i = 1; i < slx_argc(); i++)
		if (!strncmp(slx_argv(i), key, n))
			return slx_argv(i) + n;
	return NULL;
}

/*
 * With "serve=<name>": the first device the driver registered opened, taken
 * what it receives, and served, until -- with "once" -- the process ends.
 * Without, the devices are left as the driver's probe left them.
 */
static int lx_nic_task(void *unused)
{
	const char *serve = lx_nic_arg("serve=");
	struct net_device *dev, *first = NULL;
	int err = 0;

	if (!serve)
		return 0;
	rtnl_lock();
	for_each_netdev(&init_net, dev) {
		if (!(dev->flags & IFF_LOOPBACK)) {
			first = dev;
			break;
		}
	}
	if (first) {
		err = dev_open(first, NULL);
		if (!err)
			err = netdev_rx_handler_register(first, lx_nic_port_rx, NULL);
		if (err)
			netdev_err(first, "could not be opened and listened to: %d\n", err);
	}
	rtnl_unlock();
	if (!first) {
		pr_err("lx-nic: the driver registered no network device\n");
		err = -ENODEV;
	}
	if (!err)
		err = lx_nic_serve(first, serve, lx_nic_mode("once"));
	/* A warning anywhere in the process -- the driver's probe too -- is a bug. */
	if (!err && lx_warnings) {
		pr_err("lx-nic: Linux warned %lu time(s) in a run that must not\n", lx_warnings);
		slx_exit(SLX_EXIT_WARNED);
	}
	slx_exit(err ? SLX_EXIT_INIT_FAILED : SLX_EXIT_OK);
}

/* After the driver's own initcalls, which register its devices as they probe. */
static int __init lx_nic_start(void)
{
	struct task_struct *task = kthread_run(lx_nic_task, NULL, "lx-nic");

	return PTR_ERR_OR_ZERO(task);
}
late_initcall(lx_nic_start);

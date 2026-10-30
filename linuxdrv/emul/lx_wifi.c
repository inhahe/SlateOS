// SPDX-License-Identifier: MIT
/*
 * A WiFi driver process's own task: what the layer asks of Linux's 802.11
 * stack (cfg80211, mac80211) for a card the driver registered.
 *
 * With "scan": the card's wireless interface -- which mac80211 makes once
 * the driver has registered the card, after its firmware is loaded -- is
 * brought up, nl80211 is asked for a scan as wpa_supplicant or iw would ask
 * it (NL80211_CMD_TRIGGER_SCAN, through lx_netlink.c), and once its "scan"
 * group says the results are in, every network cfg80211 found is said --
 * its name, address, frequency and signal -- and the process ends: 0 for a
 * scan that finished, whatever it found. The scan is passive (no SSID
 * asked for): the card only listens, sending nothing on a network nobody
 * has chosen. Without "scan", and given no card, the process ends once its
 * initcalls are done; given a card, the task does nothing.
 *
 * And what of the kernel's key management cfg80211 asks for: a keyring
 * that trusts nothing (below).
 */
#include <linux/delay.h>
#include <linux/etherdevice.h>
#include <linux/key.h>
#include <linux/slab.h>
#include <linux/verification.h>
#include <keys/asymmetric-type.h>
#include <linux/kthread.h>
#include <linux/netdevice.h>
#include <linux/rtnetlink.h>
#include <linux/skbuff.h>
#include <net/cfg80211.h>
#include <net/genetlink.h>
#include <net/netlink.h>
#include <uapi/linux/nl80211.h>

#include "slx.h"
#include "lx_emul.h"

/*
 * The keyring cfg80211 checks a regulatory database's signature against
 * (net/wireless/reg.c: its compiled-in certificates loaded into it at
 * start-up, a database read from the firmware directory verified by them).
 * A driver process has no kernel keyring, so one that trusts nothing stands
 * in: it is made, nothing is loaded into it, and every signature checked
 * against it is rejected -- so a regulatory.db, were one staged, is
 * refused, and cfg80211 keeps to the world domain's rules, the strictest.
 */
struct key *keyring_alloc(const char *description, kuid_t uid, kgid_t gid,
			  const struct cred *cred, key_perm_t perm, unsigned long flags,
			  struct key_restriction *restrict_link, struct key *dest)
{
	struct key *keyring = kzalloc(sizeof(*keyring), GFP_KERNEL);

	return keyring ?: ERR_PTR(-ENOMEM);
}

int x509_load_certificate_list(const u8 cert_list[], const unsigned long list_size,
			       const struct key *keyring)
{
	return 0;
}

int verify_pkcs7_signature(const void *data, size_t len, const void *raw_pkcs7, size_t pkcs7_len,
			   struct key *trusted_keys, enum key_being_used_for usage,
			   int (*view_content)(void *ctx, const void *data, size_t len,
					       size_t asn1hdrlen),
			   void *ctx)
{
	return -EKEYREJECTED;
}

void key_put(struct key *key)
{
	kfree(key);
}

/* How long the card has to register, and a scan to finish. */
#define LX_WIFI_REGISTER_MS 30000
#define LX_WIFI_SCAN_S 30

/* The first wireless interface the driver's card has, or NULL; under the RTNL. */
static struct net_device *lx_wifi_dev(void)
{
	struct net_device *dev;

	for_each_netdev(&init_net, dev)
		if (dev->ieee80211_ptr)
			return dev;
	return NULL;
}

static unsigned int lx_wifi_networks;

/* One network cfg80211 has: said, its name escaped as Linux prints one. */
static void lx_wifi_bss(struct wiphy *wiphy, struct cfg80211_bss *bss, void *data)
{
	const u8 *ssid;

	rcu_read_lock();
	ssid = ieee80211_bss_get_ie(bss, WLAN_EID_SSID);
	pr_info("lx-wifi: \"%*pE\" %pM %u MHz %d dBm\n", ssid ? ssid[1] : 0, ssid ? ssid + 2 : NULL,
		bss->bssid, bss->channel->center_freq, bss->signal / 100);
	rcu_read_unlock();
	lx_wifi_networks++;
}

/*
 * The next message nl80211's "scan" group sends that ends a scan: the
 * results in, or the scan aborted -- the command it carries; 0 for none in
 * time.
 */
static u8 lx_wifi_scan_end(const struct genl_family *nl80211)
{
	unsigned long until = jiffies + LX_WIFI_SCAN_S * HZ;

	while (time_before(jiffies, until)) {
		struct sk_buff *msg = lx_genl_event(nl80211, NL80211_MULTICAST_GROUP_SCAN,
						    until - jiffies);
		u8 cmd;

		if (!msg)
			break;
		cmd = ((struct genlmsghdr *)nlmsg_data(nlmsg_hdr(msg)))->cmd;
		nlmsg_free(msg);
		if (cmd == NL80211_CMD_NEW_SCAN_RESULTS || cmd == NL80211_CMD_SCAN_ABORTED)
			return cmd;
	}
	return 0;
}

static int lx_wifi_scan(void)
{
	const struct genl_family *nl80211 = lx_genl_family(NL80211_GENL_NAME);
	struct net_device *dev = NULL;
	struct sk_buff_head replies;
	struct sk_buff *req;
	int waited, err = 0;
	u8 end;

	if (!nl80211) {
		pr_err("lx-wifi: nl80211 is not registered\n");
		return -ENODEV;
	}
	for (waited = 0; waited < LX_WIFI_REGISTER_MS; waited += 100) {
		rtnl_lock();
		dev = lx_wifi_dev();
		if (dev)
			err = dev_open(dev, NULL);
		rtnl_unlock();
		if (dev)
			break;
		msleep(100);
	}
	if (!dev) {
		pr_err("lx-wifi: no wireless interface in %d s: the card did not register\n",
		       LX_WIFI_REGISTER_MS / 1000);
		return -ENODEV;
	}
	if (err) {
		netdev_err(dev, "could not be brought up: %d\n", err);
		return err;
	}
	netdev_info(dev, "up, %pM\n", dev->dev_addr);
	err = lx_genl_listen(nl80211, NL80211_MULTICAST_GROUP_SCAN);
	if (err)
		return err;
	req = lx_genl_request(nl80211, NL80211_CMD_TRIGGER_SCAN);
	if (!req || nla_put_u32(req, NL80211_ATTR_IFINDEX, dev->ifindex)) {
		nlmsg_free(req);
		return -ENOMEM;
	}
	skb_queue_head_init(&replies);
	err = lx_genl_doit(nl80211, req, &replies);
	nlmsg_free(req);
	skb_queue_purge(&replies);
	if (err) {
		netdev_err(dev, "nl80211 would not scan: %d\n", err);
		return err;
	}
	end = lx_wifi_scan_end(nl80211);
	if (end != NL80211_CMD_NEW_SCAN_RESULTS) {
		netdev_err(dev, "the scan %s\n", end ? "was aborted" : "did not finish in time");
		return -ETIMEDOUT;
	}
	cfg80211_bss_iter(dev->ieee80211_ptr->wiphy, NULL, lx_wifi_bss, NULL);
	pr_info("lx-wifi: scan: %u network(s)\n", lx_wifi_networks);
	return 0;
}

static int lx_wifi_task(void *unused)
{
	if (lx_arg("scan"))
		slx_exit(lx_wifi_scan() ? SLX_EXIT_INIT_FAILED : SLX_EXIT_OK);
	if (lx_arg("pci="))
		return 0;
	/*
	 * Given no card there is nothing to drive: the process ends once its
	 * initcalls are done (lx_emul_main() says so) -- not when the timers
	 * the 802.11 stack set have all run, the last of them a minute out
	 * (cfg80211 checks the channels against a new regulatory domain after
	 * REG_ENFORCE_GRACE_MS).
	 */
	while (system_state != SYSTEM_RUNNING)
		msleep(10);
	slx_exit(SLX_EXIT_OK);
}

/* After the driver's own initcalls, which register its card as it probes. */
static int __init lx_wifi_start(void)
{
	struct task_struct *task = kthread_run(lx_wifi_task, NULL, "lx-wifi");

	return PTR_ERR_OR_ZERO(task);
}
late_initcall(lx_wifi_start);

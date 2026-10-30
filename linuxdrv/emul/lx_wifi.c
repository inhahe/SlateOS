// SPDX-License-Identifier: MIT
/*
 * A WiFi driver process's own task: what the layer asks of Linux's 802.11
 * stack (cfg80211, mac80211) for the radios the driver registered, through
 * nl80211 as wpa_supplicant or iw would ask it (lx_netlink.c).
 *
 * - "scan": the card's wireless interface -- which mac80211 makes once the
 *   driver has registered the card, after its firmware is loaded -- is
 *   brought up and scanned (NL80211_CMD_TRIGGER_SCAN), and once nl80211's
 *   "scan" group says the results are in, every network cfg80211 found is
 *   said -- its name, address, frequency and signal -- and the process
 *   ends: 0 for a scan that finished, whatever it found. Passive (no SSID
 *   asked for): the card only listens, sending nothing on a network nobody
 *   has chosen.
 * - "hwsim": mac80211_hwsim's two simulated radios on one medium -- the
 *   first made an open access point (NL80211_CMD_SET_INTERFACE to AP, then
 *   NL80211_CMD_START_AP, beaconing LX_WIFI_SSID on LX_WIFI_FREQ), the
 *   second scanning as above -- and the process ends 0 only if the scan
 *   found that network, from the first radio's address, on that frequency.
 *   The whole of the stack and the layer's use of it, where there is no
 *   card (the boot test).
 * - Neither, and given no card: the process ends once its initcalls are
 *   done. Given a card, the task does nothing.
 *
 * And what of the kernel's key management cfg80211 asks for: a keyring
 * that trusts nothing (below).
 */
#include <linux/delay.h>
#include <linux/etherdevice.h>
#include <linux/ieee80211.h>
#include <linux/key.h>
#include <linux/kthread.h>
#include <linux/netdevice.h>
#include <linux/rtnetlink.h>
#include <linux/skbuff.h>
#include <linux/slab.h>
#include <linux/verification.h>
#include <keys/asymmetric-type.h>
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

/* How long the radios have to register, and a scan to finish. */
#define LX_WIFI_REGISTER_MS 30000
#define LX_WIFI_SCAN_S 30

/* The network the "hwsim" run's access point beacons, and where: channel 1. */
#define LX_WIFI_SSID "slateos-hwsim"
#define LX_WIFI_FREQ 2412
#define LX_WIFI_CHANNEL 1

/*
 * The first `n` wireless interfaces, in the order mac80211 made them, once
 * there are that many -- waiting up to LX_WIFI_REGISTER_MS for the driver to
 * register its radios. False, said, when there never are.
 */
static bool lx_wifi_devs(struct net_device **devs, int n)
{
	int waited, found = 0;

	for (waited = 0; waited < LX_WIFI_REGISTER_MS; waited += 100) {
		struct net_device *dev;

		found = 0;
		rtnl_lock();
		for_each_netdev(&init_net, dev)
			if (dev->ieee80211_ptr && found < n)
				devs[found++] = dev;
		rtnl_unlock();
		if (found == n)
			return true;
		msleep(100);
	}
	pr_err("lx-wifi: %d of the %d wireless interface(s) wanted in %d s: the radios did not register\n",
	       found, n, LX_WIFI_REGISTER_MS / 1000);
	return false;
}

static int lx_wifi_open(struct net_device *dev)
{
	int err;

	rtnl_lock();
	err = dev_open(dev, NULL);
	rtnl_unlock();
	if (err)
		netdev_err(dev, "could not be brought up: %d\n", err);
	else
		netdev_info(dev, "up, %pM\n", dev->dev_addr);
	return err;
}

/*
 * nl80211's command `cmd` for `dev`'s interface, with `fill` (when not NULL)
 * adding its other attributes to the request: the handler's answer, said
 * when it is an error. The replies are not wanted.
 */
static int lx_wifi_ask(const struct genl_family *nl80211, u8 cmd, struct net_device *dev,
		       int (*fill)(struct sk_buff *req, struct net_device *dev), const char *what)
{
	struct sk_buff *req = lx_genl_request(nl80211, cmd);
	struct sk_buff_head replies;
	int err;

	if (!req || nla_put_u32(req, NL80211_ATTR_IFINDEX, dev->ifindex) ||
	    (fill && fill(req, dev))) {
		nlmsg_free(req);
		return -ENOMEM;
	}
	skb_queue_head_init(&replies);
	err = lx_genl_doit(nl80211, req, &replies);
	nlmsg_free(req);
	skb_queue_purge(&replies);
	if (err)
		netdev_err(dev, "nl80211 would not %s: %d\n", what, err);
	return err;
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

/* A passive scan on `dev`, which is up, to its end: 0, or why not. */
static int lx_wifi_scan_on(const struct genl_family *nl80211, struct net_device *dev)
{
	int err = lx_genl_listen(nl80211, NL80211_MULTICAST_GROUP_SCAN);
	u8 end;

	if (!err)
		err = lx_wifi_ask(nl80211, NL80211_CMD_TRIGGER_SCAN, dev, NULL, "scan");
	if (err)
		return err;
	end = lx_wifi_scan_end(nl80211);
	if (end != NL80211_CMD_NEW_SCAN_RESULTS) {
		netdev_err(dev, "the scan %s\n", end ? "was aborted" : "did not finish in time");
		return -ETIMEDOUT;
	}
	return 0;
}

/* What the networks a scan found are told against: the one wanted, if any. */
struct lx_wifi_found {
	unsigned int networks;
	const u8 *bssid;
	bool wanted;
};

/*
 * One network cfg80211 has: said, its name escaped as Linux prints one;
 * and whether it is the wanted one -- LX_WIFI_SSID from `bssid`, on
 * LX_WIFI_FREQ.
 */
static void lx_wifi_bss(struct wiphy *wiphy, struct cfg80211_bss *bss, void *data)
{
	struct lx_wifi_found *found = data;
	const u8 *ssid;

	rcu_read_lock();
	ssid = ieee80211_bss_get_ie(bss, WLAN_EID_SSID);
	pr_info("lx-wifi: \"%*pE\" %pM %u MHz %d dBm\n", ssid ? ssid[1] : 0, ssid ? ssid + 2 : NULL,
		bss->bssid, bss->channel->center_freq, bss->signal / 100);
	if (found->bssid && ssid && ssid[1] == strlen(LX_WIFI_SSID) &&
	    !memcmp(ssid + 2, LX_WIFI_SSID, ssid[1]) && ether_addr_equal(bss->bssid, found->bssid) &&
	    bss->channel->center_freq == LX_WIFI_FREQ)
		found->wanted = true;
	rcu_read_unlock();
	found->networks++;
}

/* The networks `dev`'s wiphy has, said and told against `found`. */
static void lx_wifi_results(struct net_device *dev, struct lx_wifi_found *found)
{
	cfg80211_bss_iter(dev->ieee80211_ptr->wiphy, NULL, lx_wifi_bss, found);
	pr_info("lx-wifi: scan: %u network(s)\n", found->networks);
}

static int lx_wifi_scan(void)
{
	const struct genl_family *nl80211 = lx_genl_family(NL80211_GENL_NAME);
	struct lx_wifi_found found = {};
	struct net_device *dev;
	int err;

	if (!nl80211 || !lx_wifi_devs(&dev, 1))
		return -ENODEV;
	err = lx_wifi_open(dev);
	if (!err)
		err = lx_wifi_scan_on(nl80211, dev);
	if (!err)
		lx_wifi_results(dev, &found);
	return err;
}

/* The access point's interface made one (NL80211_CMD_SET_INTERFACE). */
static int lx_wifi_ap_type(struct sk_buff *req, struct net_device *dev)
{
	return nla_put_u32(req, NL80211_ATTR_IFTYPE, NL80211_IFTYPE_AP);
}

/*
 * NL80211_CMD_START_AP's attributes: an open network named LX_WIFI_SSID on
 * LX_WIFI_FREQ, without HT, beaconing every 100 TU. The beacon's head is
 * the frame up to its first element and the elements cfg80211 wants there
 * -- the SSID and the rates, 1, 2, 5.5 and 11 Mb/s, all basic -- its tail
 * the channel.
 */
static int lx_wifi_ap_params(struct sk_buff *req, struct net_device *dev)
{
	static const u8 rates[] = { WLAN_EID_SUPP_RATES, 4, 0x82, 0x84, 0x8b, 0x96 };
	const u8 tail[] = { WLAN_EID_DS_PARAMS, 1, LX_WIFI_CHANNEL };
	const size_t ssid_len = strlen(LX_WIFI_SSID);
	u8 head[sizeof(struct ieee80211_hdr_3addr) + 12 + 2 + 32 + sizeof(rates)];
	struct ieee80211_mgmt *mgmt = (struct ieee80211_mgmt *)head;
	u8 *ie = mgmt->u.beacon.variable;

	memset(head, 0, sizeof(head));
	mgmt->frame_control = cpu_to_le16(IEEE80211_FTYPE_MGMT | IEEE80211_STYPE_BEACON);
	eth_broadcast_addr(mgmt->da);
	ether_addr_copy(mgmt->sa, dev->dev_addr);
	ether_addr_copy(mgmt->bssid, dev->dev_addr);
	mgmt->u.beacon.beacon_int = cpu_to_le16(100);
	mgmt->u.beacon.capab_info = cpu_to_le16(WLAN_CAPABILITY_ESS);
	*ie++ = WLAN_EID_SSID;
	*ie++ = ssid_len;
	memcpy(ie, LX_WIFI_SSID, ssid_len);
	ie += ssid_len;
	memcpy(ie, rates, sizeof(rates));
	ie += sizeof(rates);
	return nla_put(req, NL80211_ATTR_BEACON_HEAD, ie - head, head) ||
	       nla_put(req, NL80211_ATTR_BEACON_TAIL, sizeof(tail), tail) ||
	       nla_put_u32(req, NL80211_ATTR_BEACON_INTERVAL, 100) ||
	       nla_put_u32(req, NL80211_ATTR_DTIM_PERIOD, 1) ||
	       nla_put(req, NL80211_ATTR_SSID, ssid_len, LX_WIFI_SSID) ||
	       nla_put_u32(req, NL80211_ATTR_HIDDEN_SSID, NL80211_HIDDEN_SSID_NOT_IN_USE) ||
	       nla_put_u32(req, NL80211_ATTR_AUTH_TYPE, NL80211_AUTHTYPE_OPEN_SYSTEM) ||
	       nla_put_u32(req, NL80211_ATTR_WIPHY_FREQ, LX_WIFI_FREQ) ||
	       nla_put_u32(req, NL80211_ATTR_WIPHY_CHANNEL_TYPE, NL80211_CHAN_NO_HT);
}

static int lx_wifi_hwsim(void)
{
	const struct genl_family *nl80211 = lx_genl_family(NL80211_GENL_NAME);
	struct lx_wifi_found found = {};
	struct net_device *devs[2];
	int err;

	if (!nl80211 || !lx_wifi_devs(devs, 2))
		return -ENODEV;
	err = lx_wifi_ask(nl80211, NL80211_CMD_SET_INTERFACE, devs[0], lx_wifi_ap_type,
			  "make the first radio an access point");
	if (!err)
		err = lx_wifi_open(devs[0]);
	if (!err)
		err = lx_wifi_ask(nl80211, NL80211_CMD_START_AP, devs[0], lx_wifi_ap_params,
				  "start the access point");
	if (!err)
		err = lx_wifi_open(devs[1]);
	if (!err)
		err = lx_wifi_scan_on(nl80211, devs[1]);
	if (err)
		return err;
	found.bssid = devs[0]->dev_addr;
	lx_wifi_results(devs[1], &found);
	if (!found.wanted) {
		pr_err("lx-wifi: the scan did not find \"%s\" from %pM on %u MHz\n", LX_WIFI_SSID,
		       devs[0]->dev_addr, LX_WIFI_FREQ);
		return -ENOENT;
	}
	pr_info("lx-wifi: hwsim: \"%s\" found, beaconed by the first radio and heard by the second\n",
		LX_WIFI_SSID);
	return 0;
}

static int lx_wifi_task(void *unused)
{
	if (lx_arg("scan"))
		slx_exit(lx_wifi_scan() ? SLX_EXIT_INIT_FAILED : SLX_EXIT_OK);
	if (lx_arg("hwsim"))
		slx_exit(lx_wifi_hwsim() ? SLX_EXIT_INIT_FAILED : SLX_EXIT_OK);
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

/* After the driver's own initcalls, which register its radios as they probe. */
static int __init lx_wifi_start(void)
{
	struct task_struct *task = kthread_run(lx_wifi_task, NULL, "lx-wifi");

	return PTR_ERR_OR_ZERO(task);
}
late_initcall(lx_wifi_start);

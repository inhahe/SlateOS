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
 *   second scanning as above, which must find that network, from the first
 *   radio's address, on that frequency. Then the second joins it
 *   (NL80211_CMD_CONNECT), the layer answering its authentication and
 *   association on the first as hostapd would (lx_wifi_join), and a frame
 *   crosses the network each way (lx_wifi_cross); the process ends 0 only
 *   if every step did. The whole of the stack and the layer's use of it,
 *   where there is no card (the boot test).
 * - Neither, and given no card: the process ends once its initcalls are
 *   done. Given a card, the task does nothing.
 *
 * A run that ends otherwise well, but in which Linux code warned anywhere --
 * the driver, the 802.11 stack, the layer -- ends as warned (SLX_EXIT_WARNED),
 * which the kernel's self-test fails, as the e1000e process's does.
 *
 * And what of the kernel's key management cfg80211 asks for: a keyring
 * that trusts nothing (below).
 */
#include <linux/completion.h>
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
 * How long the second radio has to join it, and a frame to cross it; the
 * association id the access point gives its one station; what the frames
 * carry.
 */
#define LX_WIFI_JOIN_S 20
#define LX_WIFI_FRAME_MS 2000
#define LX_WIFI_AID 1
#define LX_WIFI_PAYLOAD "slateos-hwsim: a frame across the network"

/*
 * The first `n` wireless interfaces, in the order mac80211 made them, once
 * there are that many -- waiting up to LX_WIFI_REGISTER_MS for the driver to
 * register its radios -- each held (dev_hold), so that one unregistered
 * while the run waits on it is not freed under it: lx_wifi_put() gives them
 * back. False, said, when there never are.
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
		if (found == n) {
			for (found = 0; found < n; found++)
				dev_hold(devs[found]);
			rtnl_unlock();
			return true;
		}
		rtnl_unlock();
		msleep(100);
	}
	pr_err("lx-wifi: %d of the %d wireless interface(s) wanted in %d s: the radios did not register\n",
	       found, n, LX_WIFI_REGISTER_MS / 1000);
	return false;
}

/* What lx_wifi_devs() held, given back. */
static void lx_wifi_put(struct net_device **devs, int n)
{
	while (n--)
		dev_put(devs[n]);
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
 * adding its other attributes to the request, given `arg`: the handler's
 * answer, said -- as what `dev` would not `what` -- when it is an error.
 * The replies are not wanted.
 */
static int lx_wifi_ask(const struct genl_family *nl80211, u8 cmd, struct net_device *dev,
		       int (*fill)(struct sk_buff *req, struct net_device *dev, const void *arg),
		       const void *arg, const char *what)
{
	struct sk_buff *req = lx_genl_request(nl80211, cmd);
	struct sk_buff_head replies;
	int err;

	if (!req || nla_put_u32(req, NL80211_ATTR_IFINDEX, dev->ifindex) ||
	    (fill && fill(req, dev, arg))) {
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

/* The nl80211 command a message nl80211 sent carries. */
static u8 lx_wifi_cmd(struct sk_buff *msg)
{
	return ((struct genlmsghdr *)nlmsg_data(nlmsg_hdr(msg)))->cmd;
}

/* A message's attribute `type` (nl80211 has no header of its own); NULL for none. */
static const struct nlattr *lx_wifi_attr(struct sk_buff *msg, int type)
{
	return nlmsg_find_attr(nlmsg_hdr(msg), GENL_HDRLEN, type);
}

/*
 * The jiffies left until `until`, read once -- a tick between a test and a
 * subtraction would wrap it -- and 0 once it has passed.
 */
static unsigned long lx_wifi_left(unsigned long until)
{
	long left = (long)(until - jiffies);

	return left > 0 ? left : 0;
}

/*
 * The next message nl80211's "scan" group sends that ends a scan: the
 * results in, or the scan aborted -- the command it carries; 0 for none in
 * time.
 */
static u8 lx_wifi_scan_end(const struct genl_family *nl80211)
{
	unsigned long until = jiffies + LX_WIFI_SCAN_S * HZ;

	for (;;) {
		unsigned long left = lx_wifi_left(until);
		struct sk_buff *msg;
		u8 cmd;

		if (!left)
			break;
		msg = lx_genl_event(nl80211, NL80211_MULTICAST_GROUP_SCAN, left);
		if (!msg)
			break;
		cmd = lx_wifi_cmd(msg);
		nlmsg_free(msg);
		if (cmd == NL80211_CMD_NEW_SCAN_RESULTS || cmd == NL80211_CMD_SCAN_ABORTED)
			return cmd;
	}
	return 0;
}

/* nl80211, as cfg80211 registered it; NULL, said, when it did not. */
static const struct genl_family *lx_wifi_nl80211(void)
{
	const struct genl_family *nl80211 = lx_genl_family(NL80211_GENL_NAME);

	if (!nl80211)
		pr_err("lx-wifi: nl80211 is not registered\n");
	return nl80211;
}

/* A passive scan on `dev`, which is up, to its end: 0, or why not. */
static int lx_wifi_scan_on(const struct genl_family *nl80211, struct net_device *dev)
{
	int err = lx_genl_listen(nl80211, NL80211_MULTICAST_GROUP_SCAN);
	u8 end;

	if (!err)
		err = lx_wifi_ask(nl80211, NL80211_CMD_TRIGGER_SCAN, dev, NULL, NULL, "scan");
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
	const struct genl_family *nl80211 = lx_wifi_nl80211();
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
	lx_wifi_put(&dev, 1);
	return err;
}

/* The access point's interface made one (NL80211_CMD_SET_INTERFACE). */
static int lx_wifi_ap_type(struct sk_buff *req, struct net_device *dev, const void *unused)
{
	return nla_put_u32(req, NL80211_ATTR_IFTYPE, NL80211_IFTYPE_AP);
}

/* The rates the access point offers, as an element: 1, 2, 5.5 and 11 Mb/s, all basic. */
static const u8 lx_wifi_rates[] = { WLAN_EID_SUPP_RATES, 4, 0x82, 0x84, 0x8b, 0x96 };

/*
 * NL80211_CMD_START_AP's attributes: an open network named LX_WIFI_SSID on
 * LX_WIFI_FREQ, without HT, beaconing every 100 TU. The beacon's head is
 * the frame up to its first element and the elements cfg80211 wants there
 * -- the SSID and the rates -- its tail the channel.
 */
static int lx_wifi_ap_params(struct sk_buff *req, struct net_device *dev, const void *unused)
{
	const u8 tail[] = { WLAN_EID_DS_PARAMS, 1, LX_WIFI_CHANNEL };
	const size_t ssid_len = strlen(LX_WIFI_SSID);
	u8 head[sizeof(struct ieee80211_hdr_3addr) + 12 + 2 + 32 + sizeof(lx_wifi_rates)];
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
	memcpy(ie, lx_wifi_rates, sizeof(lx_wifi_rates));
	ie += sizeof(lx_wifi_rates);
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

/*
 * The access point's side of a join, as hostapd keeps it for an open
 * network: its interface, the one station it is taking, how far that has
 * got, and the first error met answering it.
 */
struct lx_wifi_ap {
	const struct genl_family *nl80211;
	struct net_device *dev;
	u8 sta[ETH_ALEN];
	bool authenticated;
	bool associated;
	int err;
};

/* A management frame for the access point to receive (NL80211_CMD_REGISTER_FRAME), by its type. */
static int lx_wifi_frame_type(struct sk_buff *req, struct net_device *dev, const void *type)
{
	/* Every frame of the type: an empty match. */
	static const u8 any[1];

	return nla_put_u16(req, NL80211_ATTR_FRAME_TYPE, *(const u16 *)type) ||
	       nla_put(req, NL80211_ATTR_FRAME_MATCH, 0, any);
}

/* The frame `len` bytes at `frame` to be sent (NL80211_CMD_FRAME). */
struct lx_wifi_tx {
	const void *frame;
	size_t len;
};

static int lx_wifi_frame(struct sk_buff *req, struct net_device *dev, const void *arg)
{
	const struct lx_wifi_tx *tx = arg;

	return nla_put(req, NL80211_ATTR_FRAME, tx->len, tx->frame);
}

/* A management frame of subtype `stype` from the access point to its station, its fixed fields left 0. */
static void lx_wifi_ap_header(const struct lx_wifi_ap *ap, struct ieee80211_mgmt *m, u16 stype)
{
	m->frame_control = cpu_to_le16(IEEE80211_FTYPE_MGMT | stype);
	ether_addr_copy(m->da, ap->sta);
	ether_addr_copy(m->sa, ap->dev->dev_addr);
	ether_addr_copy(m->bssid, ap->dev->dev_addr);
}

/*
 * An open system authentication's request, answered: the second frame of
 * its two, success. The station it came from is the one the access point
 * takes; another's, while it takes one, is not answered.
 */
static int lx_wifi_ap_auth(struct lx_wifi_ap *ap, const struct ieee80211_mgmt *req, size_t len)
{
	u8 buf[offsetof(struct ieee80211_mgmt, u.auth.variable)] = {};
	struct ieee80211_mgmt *resp = (struct ieee80211_mgmt *)buf;
	const struct lx_wifi_tx tx = { buf, sizeof(buf) };

	if (len < sizeof(buf) || le16_to_cpu(req->u.auth.auth_alg) != WLAN_AUTH_OPEN ||
	    le16_to_cpu(req->u.auth.auth_transaction) != 1) {
		netdev_err(ap->dev, "an authentication that is not an open system's first frame, from %pM\n",
			   req->sa);
		return -EPROTO;
	}
	if (ap->authenticated && !ether_addr_equal(req->sa, ap->sta))
		return 0;
	ether_addr_copy(ap->sta, req->sa);
	lx_wifi_ap_header(ap, resp, IEEE80211_STYPE_AUTH);
	resp->u.auth.auth_alg = cpu_to_le16(WLAN_AUTH_OPEN);
	resp->u.auth.auth_transaction = cpu_to_le16(2);
	resp->u.auth.status_code = cpu_to_le16(WLAN_STATUS_SUCCESS);
	ap->authenticated = true;
	return lx_wifi_ask(ap->nl80211, NL80211_CMD_FRAME, ap->dev, lx_wifi_frame, &tx,
			   "answer an authentication");
}

/* The station an association asks for, as NL80211_CMD_NEW_STATION adds it. */
struct lx_wifi_new_sta {
	const u8 *addr;
	const u8 *rates;
	u8 n_rates;
	u16 listen_interval;
	u16 capability;
};

static int lx_wifi_new_sta(struct sk_buff *req, struct net_device *dev, const void *arg)
{
	const struct lx_wifi_new_sta *sta = arg;

	return nla_put(req, NL80211_ATTR_MAC, ETH_ALEN, sta->addr) ||
	       nla_put(req, NL80211_ATTR_STA_SUPPORTED_RATES, sta->n_rates, sta->rates) ||
	       nla_put_u16(req, NL80211_ATTR_STA_LISTEN_INTERVAL, sta->listen_interval) ||
	       nla_put_u16(req, NL80211_ATTR_STA_AID, LX_WIFI_AID) ||
	       nla_put_u16(req, NL80211_ATTR_STA_CAPABILITY, sta->capability);
}

/* The station at `addr`, named (NL80211_CMD_GET_STATION). */
static int lx_wifi_mac(struct sk_buff *req, struct net_device *dev, const void *addr)
{
	return nla_put(req, NL80211_ATTR_MAC, ETH_ALEN, addr);
}

/* The station at `addr` let through the access point's port (NL80211_CMD_SET_STATION). */
static int lx_wifi_authorize(struct sk_buff *req, struct net_device *dev, const void *addr)
{
	const struct nl80211_sta_flag_update flags = {
		.mask = BIT(NL80211_STA_FLAG_AUTHORIZED),
		.set = BIT(NL80211_STA_FLAG_AUTHORIZED),
	};

	return lx_wifi_mac(req, dev, addr) ||
	       nla_put(req, NL80211_ATTR_STA_FLAGS2, sizeof(flags), &flags);
}

/*
 * An association's request, from the station authenticated, answered as
 * hostapd answers one on an open network: the station added with the rates
 * it asked for (NL80211_CMD_NEW_STATION), the response sent -- success, its
 * association id, the access point's rates -- and the station let through
 * the port, there being no 802.1X to wait for.
 */
static int lx_wifi_ap_assoc(struct lx_wifi_ap *ap, const struct ieee80211_mgmt *req, size_t len)
{
	const size_t fixed = offsetof(struct ieee80211_mgmt, u.assoc_req.variable);
	u8 buf[offsetof(struct ieee80211_mgmt, u.assoc_resp.variable) + sizeof(lx_wifi_rates)] = {};
	struct ieee80211_mgmt *resp = (struct ieee80211_mgmt *)buf;
	const struct lx_wifi_tx tx = { buf, sizeof(buf) };
	const size_t ssid_len = strlen(LX_WIFI_SSID);
	struct lx_wifi_new_sta sta;
	const u8 *ssid, *rates;
	int err;

	if (!ap->authenticated || !ether_addr_equal(req->sa, ap->sta) || len < fixed) {
		netdev_err(ap->dev, "an association asked by %pM, not by the station authenticated\n",
			   req->sa);
		return -EPROTO;
	}
	ssid = cfg80211_find_ie(WLAN_EID_SSID, req->u.assoc_req.variable, len - fixed);
	rates = cfg80211_find_ie(WLAN_EID_SUPP_RATES, req->u.assoc_req.variable, len - fixed);
	if (!ssid || ssid[1] != ssid_len || memcmp(ssid + 2, LX_WIFI_SSID, ssid_len) || !rates ||
	    !rates[1]) {
		netdev_err(ap->dev, "an association from %pM without this network's name, or without rates\n",
			   req->sa);
		return -EPROTO;
	}
	sta = (struct lx_wifi_new_sta){
		.addr = ap->sta,
		.rates = rates + 2,
		.n_rates = rates[1],
		.listen_interval = le16_to_cpu(req->u.assoc_req.listen_interval),
		.capability = le16_to_cpu(req->u.assoc_req.capab_info),
	};
	err = lx_wifi_ask(ap->nl80211, NL80211_CMD_NEW_STATION, ap->dev, lx_wifi_new_sta, &sta,
			  "add the station associating");
	if (err)
		return err;
	lx_wifi_ap_header(ap, resp, IEEE80211_STYPE_ASSOC_RESP);
	resp->u.assoc_resp.capab_info = cpu_to_le16(WLAN_CAPABILITY_ESS);
	resp->u.assoc_resp.status_code = cpu_to_le16(WLAN_STATUS_SUCCESS);
	/* The two top bits set, as every association id is sent. */
	resp->u.assoc_resp.aid = cpu_to_le16(LX_WIFI_AID | BIT(14) | BIT(15));
	memcpy(resp->u.assoc_resp.variable, lx_wifi_rates, sizeof(lx_wifi_rates));
	err = lx_wifi_ask(ap->nl80211, NL80211_CMD_FRAME, ap->dev, lx_wifi_frame, &tx,
			  "answer an association");
	if (!err)
		err = lx_wifi_ask(ap->nl80211, NL80211_CMD_SET_STATION, ap->dev, lx_wifi_authorize,
				  ap->sta, "let the station through");
	if (!err)
		ap->associated = true;
	return err;
}

/*
 * A message sent to the layer's port, as the access point's hostapd: a
 * registered frame for its interface answered, anything else -- the
 * statuses of the frames it sent -- passed over. The first error is kept.
 */
static void lx_wifi_ap_message(struct lx_wifi_ap *ap, struct sk_buff *msg)
{
	const struct nlattr *ifindex = lx_wifi_attr(msg, NL80211_ATTR_IFINDEX);
	const struct nlattr *frame = lx_wifi_attr(msg, NL80211_ATTR_FRAME);
	const struct ieee80211_mgmt *m;
	size_t len;
	int err;

	if (lx_wifi_cmd(msg) != NL80211_CMD_FRAME || !ifindex || !frame ||
	    nla_get_u32(ifindex) != (u32)ap->dev->ifindex || ap->err)
		return;
	m = nla_data(frame);
	len = nla_len(frame);
	/* Each answer checks its own fixed fields: here, the header they all have. */
	if (len < offsetof(struct ieee80211_mgmt, u))
		err = -EPROTO;
	else if (ieee80211_is_auth(m->frame_control))
		err = lx_wifi_ap_auth(ap, m, len);
	else if (ieee80211_is_assoc_req(m->frame_control))
		err = lx_wifi_ap_assoc(ap, m, len);
	else
		err = 0;
	ap->err = err;
}

/* What the station connecting asks (NL80211_CMD_CONNECT): the open network the scan found. */
static int lx_wifi_connect_params(struct sk_buff *req, struct net_device *dev, const void *bssid)
{
	return nla_put(req, NL80211_ATTR_SSID, strlen(LX_WIFI_SSID), LX_WIFI_SSID) ||
	       nla_put(req, NL80211_ATTR_MAC, ETH_ALEN, bssid) ||
	       nla_put_u32(req, NL80211_ATTR_WIPHY_FREQ, LX_WIFI_FREQ) ||
	       nla_put_u32(req, NL80211_ATTR_AUTH_TYPE, NL80211_AUTHTYPE_OPEN_SYSTEM);
}

/*
 * A message nl80211's "mlme" group sent: for a connection's end on `dev`
 * (NL80211_CMD_CONNECT), its status code -- WLAN_STATUS_SUCCESS for joined
 * -- else -1. One that timed out carries no code: WLAN_STATUS_UNSPECIFIED_FAILURE.
 */
static int lx_wifi_connect_status(struct sk_buff *msg, struct net_device *dev)
{
	const struct nlattr *ifindex = lx_wifi_attr(msg, NL80211_ATTR_IFINDEX);
	const struct nlattr *status = lx_wifi_attr(msg, NL80211_ATTR_STATUS_CODE);

	if (lx_wifi_cmd(msg) != NL80211_CMD_CONNECT || !ifindex ||
	    nla_get_u32(ifindex) != (u32)dev->ifindex)
		return -1;
	return status ? nla_get_u16(status) : WLAN_STATUS_UNSPECIFIED_FAILURE;
}

/*
 * The station `sta` joining the open network the access point `ap` runs:
 * `ap` registered for authentications and associations, `sta` told to
 * connect, and the frames `ap` is sent answered -- as hostapd answers them
 * -- until nl80211 says how `sta`'s connection ended, within LX_WIFI_JOIN_S.
 * 0 once `sta` is connected, its carrier on, and `ap` has it as a station;
 * else why not, said.
 */
static int lx_wifi_join(const struct genl_family *nl80211, struct net_device *ap_dev,
			struct net_device *sta_dev)
{
	static const u16 auth = IEEE80211_FTYPE_MGMT | IEEE80211_STYPE_AUTH;
	static const u16 assoc = IEEE80211_FTYPE_MGMT | IEEE80211_STYPE_ASSOC_REQ;
	struct lx_wifi_ap ap = { .nl80211 = nl80211, .dev = ap_dev };
	unsigned long until = jiffies + LX_WIFI_JOIN_S * HZ;
	int status = -1;
	int err;

	err = lx_genl_listen(nl80211, NL80211_MULTICAST_GROUP_MLME);
	if (!err)
		err = lx_wifi_ask(nl80211, NL80211_CMD_REGISTER_FRAME, ap_dev, lx_wifi_frame_type, &auth,
				  "take authentications");
	if (!err)
		err = lx_wifi_ask(nl80211, NL80211_CMD_REGISTER_FRAME, ap_dev, lx_wifi_frame_type, &assoc,
				  "take associations");
	if (!err)
		err = lx_wifi_ask(nl80211, NL80211_CMD_CONNECT, sta_dev, lx_wifi_connect_params,
				  ap_dev->dev_addr, "connect");
	while (!err && status < 0) {
		struct sk_buff *msg;

		if (!lx_genl_wait(lx_wifi_left(until))) {
			netdev_err(sta_dev, "did not join \"%s\" in %d s (the access point %s it)\n",
				   LX_WIFI_SSID, LX_WIFI_JOIN_S,
				   ap.associated ? "associated" : ap.authenticated ? "authenticated" :
										    "never heard from");
			return -ETIMEDOUT;
		}
		while ((msg = lx_genl_unicast(0))) {
			lx_wifi_ap_message(&ap, msg);
			nlmsg_free(msg);
		}
		err = ap.err;
		while ((msg = lx_genl_event(nl80211, NL80211_MULTICAST_GROUP_MLME, 0))) {
			if (status < 0)
				status = lx_wifi_connect_status(msg, sta_dev);
			nlmsg_free(msg);
		}
	}
	if (err)
		return err;
	if (status != WLAN_STATUS_SUCCESS || !ap.associated) {
		netdev_err(sta_dev, "joining \"%s\" ended with status %d, the access point %s\n",
			   LX_WIFI_SSID, status,
			   ap.associated ? "having associated it" : "not having associated it");
		return -ECONNREFUSED;
	}
	if (!netif_carrier_ok(sta_dev)) {
		netdev_err(sta_dev, "joined \"%s\", but its carrier is off\n", LX_WIFI_SSID);
		return -ENOLINK;
	}
	/* The access point has it as a station: nl80211 answers for it, -ENOENT were it not. */
	err = lx_wifi_ask(nl80211, NL80211_CMD_GET_STATION, ap_dev, lx_wifi_mac, sta_dev->dev_addr,
			  "say what it has of the station");
	if (!err)
		netdev_info(sta_dev, "joined \"%s\", %pM, as station %pM\n", LX_WIFI_SSID,
			    ap_dev->dev_addr, sta_dev->dev_addr);
	return err;
}

/*
 * A frame crossing the network: `from` sending LX_WIFI_PAYLOAD to `to`,
 * under the local experimental ethertype, and `to` receiving it -- from
 * `from`'s address, the payload whole -- within LX_WIFI_FRAME_MS.
 */
struct lx_wifi_crossing {
	struct packet_type type;
	struct net_device *from;
	struct completion arrived;
};

static int lx_wifi_rx(struct sk_buff *skb, struct net_device *dev, struct packet_type *pt,
		      struct net_device *orig_dev)
{
	struct lx_wifi_crossing *c = container_of(pt, struct lx_wifi_crossing, type);
	const size_t len = strlen(LX_WIFI_PAYLOAD);

	if (skb->len >= len && !memcmp(skb->data, LX_WIFI_PAYLOAD, len) &&
	    ether_addr_equal(eth_hdr(skb)->h_source, c->from->dev_addr))
		complete(&c->arrived);
	kfree_skb(skb);
	return NET_RX_SUCCESS;
}

static int lx_wifi_cross(struct net_device *from, struct net_device *to)
{
	struct lx_wifi_crossing c = {
		.type = { .type = htons(ETH_P_802_EX1), .dev = to, .func = lx_wifi_rx },
		.from = from,
	};
	const size_t len = strlen(LX_WIFI_PAYLOAD);
	struct sk_buff *skb;
	int err;

	init_completion(&c.arrived);
	skb = alloc_skb(LL_RESERVED_SPACE(from) + len + from->needed_tailroom, GFP_KERNEL);
	if (!skb)
		return -ENOMEM;
	skb_reserve(skb, LL_RESERVED_SPACE(from));
	skb_reset_network_header(skb);
	skb_put_data(skb, LX_WIFI_PAYLOAD, len);
	skb->dev = from;
	skb->protocol = htons(ETH_P_802_EX1);
	if (dev_hard_header(skb, from, ETH_P_802_EX1, to->dev_addr, from->dev_addr, len) < 0) {
		kfree_skb(skb);
		return -EINVAL;
	}
	dev_add_pack(&c.type);
	/*
	 * A drop is a positive code (NET_XMIT_DROP); congestion is no failure.
	 * Sent once, then judged: net_xmit_eval() is a macro that reads its
	 * argument twice, so the send in it would be made twice -- the second
	 * time of a buffer the first had already consumed.
	 */
	err = dev_queue_xmit(skb);
	err = net_xmit_eval(err);
	if (err > 0)
		err = -ENOBUFS;
	if (!err && !wait_for_completion_timeout(&c.arrived, msecs_to_jiffies(LX_WIFI_FRAME_MS)))
		err = -ETIMEDOUT;
	dev_remove_pack(&c.type);
	if (err)
		netdev_err(from, "a frame to %pM did not cross the network: %d\n", to->dev_addr, err);
	return err;
}

static int lx_wifi_hwsim(void)
{
	const struct genl_family *nl80211 = lx_wifi_nl80211();
	struct lx_wifi_found found = {};
	struct net_device *devs[2];
	int err;

	if (!nl80211 || !lx_wifi_devs(devs, 2))
		return -ENODEV;
	err = lx_wifi_ask(nl80211, NL80211_CMD_SET_INTERFACE, devs[0], lx_wifi_ap_type, NULL,
			  "make the first radio an access point");
	if (!err)
		err = lx_wifi_open(devs[0]);
	if (!err)
		err = lx_wifi_ask(nl80211, NL80211_CMD_START_AP, devs[0], lx_wifi_ap_params, NULL,
				  "start the access point");
	if (!err)
		err = lx_wifi_open(devs[1]);
	if (!err)
		err = lx_wifi_scan_on(nl80211, devs[1]);
	if (!err) {
		found.bssid = devs[0]->dev_addr;
		lx_wifi_results(devs[1], &found);
		if (found.wanted)
			pr_info("lx-wifi: hwsim: \"%s\" found, beaconed by the first radio and heard by the second\n",
				LX_WIFI_SSID);
		else
			pr_err("lx-wifi: the scan did not find \"%s\" from %pM on %u MHz\n",
			       LX_WIFI_SSID, devs[0]->dev_addr, LX_WIFI_FREQ);
		err = found.wanted ? 0 : -ENOENT;
	}
	if (!err)
		err = lx_wifi_join(nl80211, devs[0], devs[1]);
	if (!err)
		err = lx_wifi_cross(devs[1], devs[0]);
	if (!err)
		err = lx_wifi_cross(devs[0], devs[1]);
	if (!err)
		pr_info("lx-wifi: hwsim: a frame crossed \"%s\" each way\n", LX_WIFI_SSID);
	lx_wifi_put(devs, 2);
	return err;
}

static int lx_wifi_task(void *unused)
{
	/* A warning anywhere -- the driver, the 802.11 stack, the layer -- is a bug. */
	if (lx_arg("scan"))
		lx_end_run("lx-wifi", lx_wifi_scan());
	if (lx_arg("hwsim"))
		lx_end_run("lx-wifi", lx_wifi_hwsim());
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
	lx_end_run("lx-wifi", 0);
}

/* After the driver's own initcalls, which register its radios as they probe. */
static int __init lx_wifi_start(void)
{
	struct task_struct *task = kthread_run(lx_wifi_task, NULL, "lx-wifi");

	return PTR_ERR_OR_ZERO(task);
}
late_initcall(lx_wifi_start);

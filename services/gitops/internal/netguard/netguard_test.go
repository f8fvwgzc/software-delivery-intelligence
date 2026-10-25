package netguard

import (
	"context"
	"net"
	"net/netip"
	"strings"
	"testing"
)

func TestPrivateLoopbackLinkLocalAndMetadataAddressesAreForbidden(t *testing.T) {
	for ip, reason := range map[string]string{
		"127.0.0.1":              "loopback",
		"127.8.9.10":             "loopback",
		"10.0.0.5":               "private",
		"172.16.0.1":             "private",
		"172.31.255.254":         "private",
		"192.168.1.10":           "private",
		"169.254.169.254":        "link-local (cloud metadata)",
		"100.64.0.1":             "carrier-grade NAT",
		"0.0.0.0":                "unspecified",
		"255.255.255.255":        "multicast or reserved",
		"224.0.0.1":              "multicast or reserved",
		"::1":                    "loopback",
		"::":                     "unspecified",
		"fe80::1":                "link-local",
		"fd00:ec2::254":          "unique local (private)",
		"::ffff:127.0.0.1":       "loopback",
		"::ffff:10.1.2.3":        "private",
		"::ffff:169.254.169.254": "link-local (cloud metadata)",
		"64:ff9b::a00:1":         "private",
	} {
		if got := ForbiddenIP(netip.MustParseAddr(ip)); got != reason {
			t.Errorf("ForbiddenIP(%s) = %q, want %q", ip, got, reason)
		}
	}
	for _, ip := range []string{"140.82.112.3", "8.8.8.8", "172.32.0.1", "192.169.0.1", "100.128.0.1", "2606:4700::1111", "::ffff:8.8.8.8"} {
		if got := ForbiddenIP(netip.MustParseAddr(ip)); got != "" {
			t.Errorf("ForbiddenIP(%s) = %q, want public", ip, got)
		}
	}
}

func TestTheDefaultPolicyRefusesEverythingThatIsNotPublicHTTPS(t *testing.T) {
	g := New(Policy{})
	ctx := context.Background()
	for _, u := range []string{
		"https://127.0.0.1:6443", "https://localhost:6443", "https://10.1.2.3", "https://192.168.0.7:6443",
		"https://169.254.169.254", "https://[::1]:6443", "https://[fd00:ec2::254]", "https://2130706433",
	} {
		if _, err := g.Check(ctx, u); err == nil {
			t.Errorf("%s was accepted", u)
		}
	}
	if _, err := g.Check(ctx, "http://8.8.8.8"); err == nil || !strings.Contains(err.Error(), "must use https") {
		t.Errorf("plain http to a public address: %v", err)
	}
	if _, err := g.Check(ctx, "https://user:pw@8.8.8.8"); err == nil || !strings.Contains(err.Error(), "credentials") {
		t.Errorf("credentials in the URL: %v", err)
	}
	for _, u := range []string{"ftp://8.8.8.8", "not a url", "https://"} {
		if _, err := g.Check(ctx, u); err == nil {
			t.Errorf("%q was accepted", u)
		}
	}
	if _, err := g.Check(ctx, "https://8.8.8.8:6443"); err != nil {
		t.Errorf("a public https server was refused: %v", err)
	}
	msg := ""
	if _, err := g.Check(ctx, "https://127.0.0.1:6443"); err != nil {
		msg = err.Error()
	}
	if !strings.Contains(msg, AllowPrivateEnv) {
		t.Errorf("the refusal must say how to allow private clusters: %q", msg)
	}
}

func TestPrivateTargetsNeedTheFlagAndHTTPStaysPrivate(t *testing.T) {
	g := New(Policy{AllowPrivate: true})
	ctx := context.Background()
	for _, u := range []string{"https://127.0.0.1:6443", "http://localhost:8080", "https://10.1.2.3", "https://8.8.8.8"} {
		if _, err := g.Check(ctx, u); err != nil {
			t.Errorf("%s refused: %v", u, err)
		}
	}
	if _, err := g.Check(ctx, "http://8.8.8.8"); err == nil {
		t.Error("http to a public address must stay refused")
	}
}

func TestTheDialerRefusesInwardAddressesAtConnectionTime(t *testing.T) {
	ln, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	defer ln.Close()
	go func() {
		for {
			c, err := ln.Accept()
			if err != nil {
				return
			}
			_ = c.Close()
		}
	}()
	_, port, _ := net.SplitHostPort(ln.Addr().String())
	if _, err := New(Policy{}).DialContext(context.Background(), "tcp", "localhost:"+port); err == nil || !strings.Contains(err.Error(), "loopback") {
		t.Fatalf("loopback must be refused when dialing: %v", err)
	}
	conn, err := New(Policy{AllowPrivate: true}).DialContext(context.Background(), "tcp", "localhost:"+port)
	if err != nil {
		t.Fatalf("allowed private dial failed: %v", err)
	}
	_ = conn.Close()
}

func TestLiteralRefusalAndRedaction(t *testing.T) {
	for u, want := range map[string]string{
		"http://169.254.169.254/latest": "link-local",
		"https://127.0.0.1:9000":        "loopback",
		"https://localhost:6443":        "the machine SDI runs on",
	} {
		if got := LiteralRefusal(u); !strings.Contains(got, want) {
			t.Errorf("LiteralRefusal(%s) = %q", u, got)
		}
	}
	if got := LiteralRefusal("https://k8s.example.com"); got != "" {
		t.Errorf("names are judged when resolved: %q", got)
	}
	if got := Redact("https://u:p@k8s.example.com/x?token=s#f"); got != "https://k8s.example.com/x" {
		t.Errorf("Redact = %s", got)
	}
}

func TestPolicyFromEnv(t *testing.T) {
	t.Setenv(AllowPrivateEnv, "1")
	if !PolicyFromEnv().AllowPrivate {
		t.Error("1 allows")
	}
	t.Setenv(AllowPrivateEnv, "0")
	if PolicyFromEnv().AllowPrivate {
		t.Error("0 refuses")
	}
}

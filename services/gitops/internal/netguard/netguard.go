// Package netguard is the server-side request forgery guard of CONTRACT.md
// §22, mirroring services/execution/src/adapters/net.rs, applied here to the
// Kubernetes API servers named by cluster kubeconfigs.
//
// A kubeconfig is typed by a workspace admin, so the service must not let
// it reach what only the service can reach: its own loopback, the private
// network it runs in, or a cloud metadata endpoint. The rule: https only,
// and every address the host resolves to must be public.
// SDI_ALLOW_PRIVATE_DEPLOY_TARGETS=1 lifts both for self-hosted installs and
// local clusters (http is then accepted for private addresses only).
//
// Two layers enforce it, as in the execution service: Check validates the
// URL before a cluster is used, and DialContext applies the same address
// test when a connection is opened, so a name that answers a public address
// to the check and a private one to the connection is refused too.
package netguard

import (
	"context"
	"fmt"
	"net"
	"net/netip"
	"net/url"
	"os"
	"strings"
	"time"
)

// AllowPrivateEnv is the variable that lifts the guard.
const AllowPrivateEnv = "SDI_ALLOW_PRIVATE_DEPLOY_TARGETS"

const resolveTimeout = 5 * time.Second

// Policy says whether deploy targets on private networks are reachable.
type Policy struct {
	AllowPrivate bool
}

// PolicyFromEnv reads SDI_ALLOW_PRIVATE_DEPLOY_TARGETS (1, true, TRUE, yes).
func PolicyFromEnv() Policy {
	switch strings.TrimSpace(os.Getenv(AllowPrivateEnv)) {
	case "1", "true", "TRUE", "yes":
		return Policy{AllowPrivate: true}
	}
	return Policy{}
}

// ForbiddenIP says why an address is not a public one, or "" when it is.
func ForbiddenIP(ip netip.Addr) string {
	ip = ip.WithZone("")
	if ip.Is4In6() {
		return forbiddenV4(ip.Unmap())
	}
	if ip.Is4() {
		return forbiddenV4(ip)
	}
	return forbiddenV6(ip)
}

func forbiddenV4(ip netip.Addr) string {
	o := ip.As4()
	a, b, c := o[0], o[1], o[2]
	switch {
	case ip.IsUnspecified() || a == 0:
		return "unspecified"
	case ip.IsLoopback():
		return "loopback"
	case ip.IsLinkLocalUnicast():
		// 169.254.0.0/16, which includes the 169.254.169.254 metadata endpoint.
		return "link-local (cloud metadata)"
	case ip.IsPrivate():
		return "private"
	case a == 100 && b >= 64 && b < 128:
		return "carrier-grade NAT"
	case a == 192 && b == 0 && c == 0:
		return "IETF protocol assignment"
	case a == 198 && (b == 18 || b == 19):
		return "benchmarking"
	case a == 255 && b == 255 && c == 255 && o[3] == 255, ip.IsMulticast(), a >= 240:
		return "multicast or reserved"
	case (a == 192 && b == 0 && c == 2) || (a == 198 && b == 51 && c == 100) || (a == 203 && b == 0 && c == 113):
		return "documentation"
	}
	return ""
}

func forbiddenV6(ip netip.Addr) string {
	s := ip.As16()
	first := uint16(s[0])<<8 | uint16(s[1])
	second := uint16(s[2])<<8 | uint16(s[3])
	switch {
	case ip.IsUnspecified():
		return "unspecified"
	case ip.IsLoopback():
		return "loopback"
	case first&0xffc0 == 0xfe80:
		return "link-local"
	case first&0xfe00 == 0xfc00:
		// fc00::/7, which includes the fd00:ec2::254 metadata endpoint.
		return "unique local (private)"
	case ip.IsMulticast():
		return "multicast"
	case first == 0x2001 && second == 0x0db8:
		return "documentation"
	case first == 0x0064 && second == 0xff9b:
		// NAT64: the embedded IPv4 address decides.
		return forbiddenV4(netip.AddrFrom4([4]byte{s[12], s[13], s[14], s[15]}))
	}
	return ""
}

// LiteralRefusal says why a URL is refused before any lookup: its host is
// an IP literal that is not public, or localhost. "" for names that still
// have to be resolved and for text that is not a URL.
func LiteralRefusal(raw string) string {
	u, err := url.Parse(strings.TrimSpace(raw))
	if err != nil || u.Host == "" {
		return ""
	}
	host := strings.TrimSuffix(strings.ToLower(u.Hostname()), ".")
	if host == "localhost" || strings.HasSuffix(host, ".localhost") {
		return host + " is the machine SDI runs on"
	}
	ip, err := netip.ParseAddr(host)
	if err != nil {
		return ""
	}
	if why := ForbiddenIP(ip); why != "" {
		return fmt.Sprintf("%s is a %s address", ip, why)
	}
	return ""
}

// Guard applies a policy to URLs and connections.
type Guard struct {
	Policy   Policy
	resolver *net.Resolver
}

// New is a guard that resolves with the system resolver.
func New(policy Policy) *Guard {
	return &Guard{Policy: policy, resolver: net.DefaultResolver}
}

// Check validates raw against the policy: scheme, no credentials in the
// URL, and where its host points. The error is a sentence for the person
// who configured the cluster.
func (g *Guard) Check(ctx context.Context, raw string) (*url.URL, error) {
	u, err := url.Parse(strings.TrimSpace(raw))
	if err != nil {
		return nil, fmt.Errorf("%s is not a valid URL", Redact(raw))
	}
	if u.User != nil {
		return nil, fmt.Errorf("API server URLs must not carry credentials")
	}
	host := u.Hostname()
	if host == "" {
		return nil, fmt.Errorf("%s has no host", Redact(raw))
	}
	addresses, err := g.addresses(ctx, host)
	if err != nil {
		return nil, err
	}
	var blockedIP netip.Addr
	blockedWhy := ""
	allPrivate := true
	for _, ip := range addresses {
		why := ForbiddenIP(ip)
		if why == "" {
			allPrivate = false
		} else if blockedWhy == "" {
			blockedIP, blockedWhy = ip, why
		}
	}
	switch u.Scheme {
	case "https":
	case "http":
		// Plain http never leaves the private network, and only when
		// private targets are allowed at all.
		if !(g.Policy.AllowPrivate && allPrivate) {
			return nil, fmt.Errorf("%s uses http; API servers must use https (http is only accepted for private addresses when %s=1)", Redact(raw), AllowPrivateEnv)
		}
	default:
		return nil, fmt.Errorf("%s URLs are not supported; API servers must use https", u.Scheme)
	}
	if !g.Policy.AllowPrivate && blockedWhy != "" {
		return nil, fmt.Errorf("%s resolves to %s, a %s address; clusters must be public (set %s=1 on the gitops service to allow private ones)", host, blockedIP, blockedWhy, AllowPrivateEnv)
	}
	return u, nil
}

func (g *Guard) addresses(ctx context.Context, host string) ([]netip.Addr, error) {
	if ip, err := netip.ParseAddr(strings.Trim(host, "[]")); err == nil {
		return []netip.Addr{ip}, nil
	}
	ctx, cancel := context.WithTimeout(ctx, resolveTimeout)
	defer cancel()
	resolved, err := g.resolver.LookupNetIP(ctx, "ip", host)
	if err != nil {
		if ctx.Err() != nil {
			return nil, fmt.Errorf("resolving %s timed out", host)
		}
		return nil, fmt.Errorf("%s does not resolve", host)
	}
	if len(resolved) == 0 {
		return nil, fmt.Errorf("%s does not resolve", host)
	}
	return resolved, nil
}

// DialContext dials only addresses the policy allows: the connection-time
// half of the guard, for http transports (client-go's rest.Config.Dial).
func (g *Guard) DialContext(ctx context.Context, network, address string) (net.Conn, error) {
	host, port, err := net.SplitHostPort(address)
	if err != nil {
		return nil, err
	}
	addresses, err := g.addresses(ctx, host)
	if err != nil {
		return nil, err
	}
	if !g.Policy.AllowPrivate {
		for _, ip := range addresses {
			if why := ForbiddenIP(ip); why != "" {
				return nil, fmt.Errorf("%s resolves to %s, a %s address; clusters must be public", host, ip, why)
			}
		}
	}
	dialer := &net.Dialer{Timeout: 10 * time.Second, KeepAlive: 30 * time.Second}
	var lastErr error
	for _, ip := range addresses {
		conn, err := dialer.DialContext(ctx, network, net.JoinHostPort(ip.String(), port))
		if err == nil {
			return conn, nil
		}
		lastErr = err
	}
	return nil, lastErr
}

// Redact is the URL without its query, fragment and credentials, for
// messages.
func Redact(raw string) string {
	u, err := url.Parse(strings.TrimSpace(raw))
	if err != nil {
		return "the URL"
	}
	u.User = nil
	u.RawQuery = ""
	u.Fragment = ""
	return u.String()
}

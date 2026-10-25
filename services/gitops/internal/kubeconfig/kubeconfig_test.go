package kubeconfig

import (
	"strings"
	"testing"
)

const base = `apiVersion: v1
kind: Config
current-context: dev
clusters:
- name: dev
  cluster:
    server: https://k8s.example.com:6443
    certificate-authority-data: Q0E=
contexts:
- name: dev
  context:
    cluster: dev
    user: dev
    namespace: shop
- name: other
  context:
    cluster: dev
    user: other
users:
- name: dev
  user:
    token: secret-token-value
- name: other
  user:
    token: other-token
`

func TestAValidKubeconfigParses(t *testing.T) {
	p, err := Parse([]byte(base), "")
	if err != nil {
		t.Fatal(err)
	}
	if p.Server != "https://k8s.example.com:6443" || p.Namespace != "shop" || p.Context != "dev" {
		t.Fatalf("parsed %+v", p)
	}
	if p.Config.BearerToken != "secret-token-value" || p.Config.Host != "https://k8s.example.com:6443" {
		t.Fatalf("config %s", p.Config.Host)
	}
	other, err := Parse([]byte(base), "other")
	if err != nil || other.Config.BearerToken != "other-token" {
		t.Fatalf("named context: %v", err)
	}
	if _, err := Parse([]byte(base), "missing"); err == nil || !strings.Contains(err.Error(), "no context") {
		t.Fatalf("missing context: %v", err)
	}
}

func TestPluginsFilesAndProxiesAreRefused(t *testing.T) {
	cases := map[string]struct{ old, new, want string }{
		"exec plugin":    {"    token: secret-token-value\n", "    exec:\n      apiVersion: client.authentication.k8s.io/v1\n      command: /bin/sh\n      args: [-c, id]\n", "exec plugin"},
		"auth provider":  {"    token: secret-token-value\n", "    auth-provider:\n      name: oidc\n      config: {}\n", "auth-provider"},
		"token file":     {"    token: secret-token-value\n", "    tokenFile: /var/run/secrets/kubernetes.io/serviceaccount/token\n", "token from a file"},
		"cert file":      {"    token: secret-token-value\n", "    client-certificate: /etc/ssl/client.crt\n    client-key: /etc/ssl/client.key\n", "from a file"},
		"no credentials": {"    token: secret-token-value\n", "    username: only-a-name\n", "no inline credentials"},
		"ca file":        {"    certificate-authority-data: Q0E=\n", "    certificate-authority: /etc/ssl/ca.crt\n", "CA from a file"},
		"proxy":          {"    certificate-authority-data: Q0E=\n", "    certificate-authority-data: Q0E=\n    proxy-url: http://10.0.0.1:3128\n", "proxy-url"},
	}
	for name, c := range cases {
		t.Run(name, func(t *testing.T) {
			raw := strings.Replace(base, c.old, c.new, 1)
			_, err := Parse([]byte(raw), "dev")
			if err == nil || !strings.Contains(err.Error(), c.want) {
				t.Fatalf("got %v, want %q", err, c.want)
			}
			if strings.Contains(err.Error(), "secret-token-value") {
				t.Fatal("an error quoted the kubeconfig")
			}
		})
	}
}

func TestGarbageIsReportedWithoutQuotingIt(t *testing.T) {
	for _, raw := range []string{"", "   ", "token: hunter2\n  : : :", "{not yaml"} {
		_, err := Parse([]byte(raw), "")
		if err == nil {
			t.Fatalf("%q parsed", raw)
		}
		if strings.Contains(err.Error(), "hunter2") {
			t.Fatal("an error quoted the input")
		}
	}
	noCurrent := strings.Replace(base, "current-context: dev\n", "", 1)
	if _, err := Parse([]byte(noCurrent), ""); err == nil || !strings.Contains(err.Error(), "no current context") {
		t.Fatalf("no current context: %v", err)
	}
}

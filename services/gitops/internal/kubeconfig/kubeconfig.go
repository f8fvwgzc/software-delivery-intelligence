// Package kubeconfig turns a workspace secret's kubeconfig into a client
// configuration, refusing everything that would make the service run a
// program, read its own files or route through somewhere else
// (CONTRACT.md §24.2). The kubeconfig only ever lives in memory here; no
// error message quotes it.
package kubeconfig

import (
	"fmt"
	"strings"
	"time"

	"k8s.io/client-go/rest"
	"k8s.io/client-go/tools/clientcmd"
	clientcmdapi "k8s.io/client-go/tools/clientcmd/api"
)

// Parsed is a usable kubeconfig.
type Parsed struct {
	// Config connects to the API server. Its credentials are only in memory.
	Config *rest.Config
	// Server is the API server URL, for display and the address guard.
	Server string
	// Namespace is the context's namespace, if it names one.
	Namespace string
	// Context is the context that was used.
	Context string
}

// Parse reads raw (YAML) and selects contextName (empty: the current
// context). Credentials must be inline: exec plugins, auth-provider plugins,
// token files and certificate or key files are refused, as is proxy-url.
func Parse(raw []byte, contextName string) (*Parsed, error) {
	if len(strings.TrimSpace(string(raw))) == 0 {
		return nil, fmt.Errorf("the kubeconfig secret is empty")
	}
	cfg, err := clientcmd.Load(raw)
	if err != nil {
		return nil, fmt.Errorf("the kubeconfig secret is not a valid kubeconfig")
	}
	name := strings.TrimSpace(contextName)
	if name == "" {
		name = cfg.CurrentContext
	}
	if name == "" {
		return nil, fmt.Errorf("the kubeconfig has no current context; name one in the cluster")
	}
	kctx, ok := cfg.Contexts[name]
	if !ok || kctx == nil {
		return nil, fmt.Errorf("the kubeconfig has no context %q", name)
	}
	cluster, ok := cfg.Clusters[kctx.Cluster]
	if !ok || cluster == nil {
		return nil, fmt.Errorf("context %q names a cluster the kubeconfig does not define", name)
	}
	auth := cfg.AuthInfos[kctx.AuthInfo]
	if err := checkCluster(kctx.Cluster, cluster); err != nil {
		return nil, err
	}
	if auth == nil {
		return nil, fmt.Errorf("context %q names a user the kubeconfig does not define", name)
	}
	if err := checkAuth(kctx.AuthInfo, auth); err != nil {
		return nil, err
	}
	// Only the selected context's entries are kept, so nothing else in the
	// file can influence the client.
	minimal := clientcmdapi.NewConfig()
	minimal.Clusters[kctx.Cluster] = cluster
	minimal.AuthInfos[kctx.AuthInfo] = auth
	minimal.Contexts[name] = kctx
	minimal.CurrentContext = name
	restConfig, err := clientcmd.NewNonInteractiveClientConfig(*minimal, name, &clientcmd.ConfigOverrides{}, nil).ClientConfig()
	if err != nil {
		return nil, fmt.Errorf("the kubeconfig's context %q is not usable: %s", name, safeMessage(err))
	}
	restConfig.WarningHandler = rest.NoWarnings{}
	restConfig.QPS = 50
	restConfig.Burst = 100
	restConfig.Timeout = 0
	restConfig.UserAgent = "sdi-gitops"
	return &Parsed{Config: restConfig, Server: cluster.Server, Namespace: kctx.Namespace, Context: name}, nil
}

func checkCluster(name string, c *clientcmdapi.Cluster) error {
	if strings.TrimSpace(c.Server) == "" {
		return fmt.Errorf("cluster %q in the kubeconfig has no server", name)
	}
	if c.CertificateAuthority != "" {
		return fmt.Errorf("cluster %q reads its CA from a file; put it inline as certificate-authority-data", name)
	}
	if c.ProxyURL != "" {
		return fmt.Errorf("cluster %q sets proxy-url, which is not supported", name)
	}
	return nil
}

func checkAuth(name string, a *clientcmdapi.AuthInfo) error {
	switch {
	case a.Exec != nil:
		return fmt.Errorf("user %q in the kubeconfig runs an exec plugin; SDI only accepts inline credentials (token, client certificate, basic auth)", name)
	case a.AuthProvider != nil:
		return fmt.Errorf("user %q in the kubeconfig uses an auth-provider plugin; SDI only accepts inline credentials", name)
	case a.TokenFile != "":
		return fmt.Errorf("user %q reads its token from a file; put it inline as token", name)
	case a.ClientCertificate != "" || a.ClientKey != "":
		return fmt.Errorf("user %q reads its client certificate or key from a file; put them inline as client-certificate-data and client-key-data", name)
	}
	// A user without credentials is refused rather than trusted: the
	// engine's apply path treats a credential-less configuration as
	// in-cluster and would present the service's own service-account token
	// to whatever server the kubeconfig names.
	hasCert := len(a.ClientCertificateData) > 0 && len(a.ClientKeyData) > 0
	hasToken := strings.TrimSpace(a.Token) != ""
	hasBasic := a.Username != "" && a.Password != ""
	if !hasCert && !hasToken && !hasBasic {
		return fmt.Errorf("user %q in the kubeconfig has no inline credentials (token, client-certificate-data with client-key-data, or username and password)", name)
	}
	return nil
}

// safeMessage is a client-go configuration error without any value it may
// echo back.
func safeMessage(err error) string {
	msg := err.Error()
	if strings.Contains(msg, "-----BEGIN") || len(msg) > 200 {
		return "invalid credentials"
	}
	return msg
}

// WithTimeout is a copy of cfg whose requests time out after d (a
// connection check).
func WithTimeout(cfg *rest.Config, d time.Duration) *rest.Config {
	out := rest.CopyConfig(cfg)
	out.Timeout = d
	return out
}

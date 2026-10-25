// Package clusters holds one gitops-engine cluster cache per cluster
// (CONTRACT.md §24.2): the informers on every watched kind that make
// comparisons cheap. A cache is built on first use from the cluster's
// kubeconfig (resolved from identity, kept in memory only), dropped when the
// cluster changes or after 30 minutes unused, and at most GITOPS_MAX_CLUSTERS
// exist at once.
package clusters

import (
	"context"
	"fmt"
	"log/slog"
	"strings"
	"sync"
	"time"

	"github.com/go-logr/logr"
	"github.com/google/uuid"
	"google.golang.org/grpc/status"
	apierrors "k8s.io/apimachinery/pkg/api/errors"
	"k8s.io/apimachinery/pkg/apis/meta/v1/unstructured"
	"k8s.io/client-go/discovery"
	"k8s.io/client-go/rest"

	"github.com/argoproj/argo-cd/gitops-engine/pkg/cache"
	"github.com/argoproj/argo-cd/gitops-engine/pkg/utils/kube"
	"github.com/argoproj/argo-cd/gitops-engine/pkg/utils/tracing"

	"github.com/f8fvwgzc/software-delivery-intelligence/services/gitops/internal/kubeconfig"
	"github.com/f8fvwgzc/software-delivery-intelligence/services/gitops/internal/netguard"
	"github.com/f8fvwgzc/software-delivery-intelligence/services/gitops/internal/store"
	"github.com/f8fvwgzc/software-delivery-intelligence/services/gitops/internal/tracking"
)

const (
	// IdleTimeout drops a cache nobody used for this long.
	IdleTimeout = 30 * time.Minute
	// SyncTimeout bounds how long a caller waits for a cache to fill.
	SyncTimeout  = 90 * time.Second
	checkTimeout = 10 * time.Second
)

// SecretResolver reads a workspace secret (identity's ResolveSecret).
type SecretResolver interface {
	ResolveSecret(ctx context.Context, workspace uuid.UUID, name string) (string, error)
}

// Error is a cluster that cannot be used, as a sentence.
type Error struct {
	Message string
	// Busy is a manager at capacity with every cache in use.
	Busy bool
}

func (e *Error) Error() string { return e.Message }

// ResourceInfo is what the cache keeps beside every resource.
type ResourceInfo struct {
	// AppID is the application whose tracking id names this very resource.
	AppID string
}

// AppOf is the application a cached resource belongs to.
func AppOf(r *cache.Resource) string {
	if info, ok := r.Info.(*ResourceInfo); ok {
		return info.AppID
	}
	return ""
}

// excluded are kinds no application manages and that change all the time;
// watching them would only cost memory and API traffic (Argo CD excludes
// the same by default).
type excluded struct{}

func (excluded) IsExcludedResource(group, kind, _ string) bool {
	switch group {
	case "events.k8s.io", "metrics.k8s.io":
		return true
	case "":
		return kind == "Event" || kind == "Endpoints"
	case "coordination.k8s.io":
		return kind == "Lease"
	case "discovery.k8s.io":
		return kind == "EndpointSlice"
	case "authentication.k8s.io", "authorization.k8s.io":
		return true
	case "certificates.k8s.io":
		return kind == "CertificateSigningRequest"
	}
	return false
}

type entry struct {
	key      string
	cluster  store.Cluster
	config   *rest.Config
	cache    cache.ClusterCache
	users    int
	lastUsed time.Time
	stale    bool
	built    chan struct{}
	buildErr error
}

// Manager owns the caches.
type Manager struct {
	secrets   SecretResolver
	guard     *netguard.Guard
	max       int
	log       *slog.Logger
	engineLog logr.Logger
	kubectl   kube.Kubectl

	mu      sync.Mutex
	entries map[uuid.UUID]*entry
}

// NewManager builds a manager of at most max caches.
func NewManager(secrets SecretResolver, guard *netguard.Guard, max int, log *slog.Logger, engineLog logr.Logger) *Manager {
	return &Manager{
		secrets:   secrets,
		guard:     guard,
		max:       max,
		log:       log,
		engineLog: engineLog,
		kubectl:   &kube.KubectlCmd{Log: engineLog, Tracer: tracing.NopTracer{}},
		entries:   map[uuid.UUID]*entry{},
	}
}

// Kubectl is the engine's kubectl wrapper every operation uses.
func (m *Manager) Kubectl() kube.Kubectl {
	return m.kubectl
}

// Handle is a cache in use; Release returns it.
type Handle struct {
	m    *Manager
	e    *entry
	once sync.Once
}

// Cache is the engine's cluster cache.
func (h *Handle) Cache() cache.ClusterCache { return h.e.cache }

// Config is the guarded client configuration.
func (h *Handle) Config() *rest.Config { return h.e.config }

// Cluster is the cluster as it was when the cache was built.
func (h *Handle) Cluster() store.Cluster { return h.e.cluster }

// Release gives the cache back.
func (h *Handle) Release() {
	h.once.Do(func() { h.m.release(h.e) })
}

// ServerVersion is the cluster's Kubernetes version ("v1.31.0").
func (h *Handle) ServerVersion() string {
	return h.e.cache.GetServerVersion()
}

// APIVersions are the cluster's group/versions and group/version/kinds, as
// Helm's .Capabilities.APIVersions wants them.
func (h *Handle) APIVersions() []string {
	seen := map[string]bool{}
	var out []string
	for _, api := range h.e.cache.GetAPIResources() {
		gv := api.GroupVersionResource.GroupVersion().String()
		for _, v := range []string{gv, gv + "/" + api.GroupKind.Kind} {
			if !seen[v] {
				seen[v] = true
				out = append(out, v)
			}
		}
	}
	return out
}

// Acquire returns the cluster's cache, building it on first use and
// waiting until it holds the cluster's state.
func (m *Manager) Acquire(ctx context.Context, c store.Cluster) (*Handle, error) {
	m.mu.Lock()
	e := m.entries[c.ID]
	if e != nil && (e.key != c.Fingerprint() || e.cluster.WorkspaceID != c.WorkspaceID) {
		m.dropLocked(c.ID)
		e = nil
	}
	build := false
	if e == nil {
		if len(m.entries) >= m.max && !m.evictLocked() {
			m.mu.Unlock()
			return nil, &Error{Message: fmt.Sprintf("%d clusters are in use at once, the limit (GITOPS_MAX_CLUSTERS); try again shortly", m.max), Busy: true}
		}
		e = &entry{key: c.Fingerprint(), cluster: c, built: make(chan struct{})}
		m.entries[c.ID] = e
		build = true
	}
	e.users++
	e.lastUsed = time.Now()
	m.mu.Unlock()

	if build {
		e.config, e.cache, e.buildErr = m.build(ctx, c)
		close(e.built)
	}
	select {
	case <-e.built:
	case <-ctx.Done():
		m.release(e)
		return nil, ctx.Err()
	}
	if e.buildErr != nil {
		m.mu.Lock()
		if m.entries[c.ID] == e {
			delete(m.entries, c.ID)
		}
		m.mu.Unlock()
		m.release(e)
		return nil, e.buildErr
	}
	if err := m.ensureSynced(ctx, e); err != nil {
		m.release(e)
		return nil, err
	}
	return &Handle{m: m, e: e}, nil
}

func (m *Manager) release(e *entry) {
	m.mu.Lock()
	defer m.mu.Unlock()
	e.users--
	e.lastUsed = time.Now()
	if e.users <= 0 && e.stale && e.cache != nil {
		e.cache.Invalidate()
	}
}

// ensureSynced fills (or refills, after an error and the engine's retry
// delay) the cache, bounded by SyncTimeout.
func (m *Manager) ensureSynced(ctx context.Context, e *entry) error {
	done := make(chan error, 1)
	go func() { done <- e.cache.EnsureSynced() }()
	timer := time.NewTimer(SyncTimeout)
	defer timer.Stop()
	select {
	case err := <-done:
		if err == nil {
			return nil
		}
		if isAuthError(err) {
			// The kubeconfig may have been rotated: resolve it again next time.
			m.Drop(e.cluster.ID)
		}
		return &Error{Message: fmt.Sprintf("cluster %s is unreachable: %s", e.cluster.Name, describe(err))}
	case <-timer.C:
		return &Error{Message: fmt.Sprintf("cluster %s did not answer within %s", e.cluster.Name, SyncTimeout)}
	case <-ctx.Done():
		return ctx.Err()
	}
}

func isAuthError(err error) bool {
	return apierrors.IsUnauthorized(err) || strings.Contains(err.Error(), "Unauthorized")
}

// Connect resolves the cluster's kubeconfig, checks the API server address
// and builds its client configuration.
func (m *Manager) Connect(ctx context.Context, c store.Cluster) (*rest.Config, string, error) {
	raw, err := m.secrets.ResolveSecret(ctx, c.WorkspaceID, c.KubeconfigSecret)
	if err != nil {
		return nil, "", &Error{Message: secretError(c.KubeconfigSecret, err)}
	}
	parsed, err := kubeconfig.Parse([]byte(raw), c.Context)
	raw = ""
	if err != nil {
		return nil, "", &Error{Message: err.Error()}
	}
	if _, err := m.guard.Check(ctx, parsed.Server); err != nil {
		return nil, parsed.Server, &Error{Message: err.Error()}
	}
	cfg := parsed.Config
	cfg.Dial = m.guard.DialContext
	return cfg, parsed.Server, nil
}

// Check reaches the cluster once (its server version), as UpsertCluster
// reports: the API server URL and version, or why it cannot be used.
func (m *Manager) Check(ctx context.Context, c store.Cluster) (server, version string, err error) {
	cfg, server, err := m.Connect(ctx, c)
	if err != nil {
		return server, "", err
	}
	disco, err := discovery.NewDiscoveryClientForConfig(kubeconfig.WithTimeout(cfg, checkTimeout))
	if err != nil {
		return server, "", &Error{Message: "the kubeconfig could not be used: " + describe(err)}
	}
	info, err := disco.ServerVersion()
	if err != nil {
		return server, "", &Error{Message: "the cluster did not answer: " + describe(err)}
	}
	return server, info.GitVersion, nil
}

func (m *Manager) build(ctx context.Context, c store.Cluster) (*rest.Config, cache.ClusterCache, error) {
	cfg, _, err := m.Connect(ctx, c)
	if err != nil {
		return nil, nil, err
	}
	opts := []cache.UpdateSettingsFunc{
		cache.SetLogr(m.engineLog),
		cache.SetKubectl(m.kubectl),
		cache.SetSettings(cache.Settings{ResourceHealthOverride: cache.NewNoopSettings(), ResourcesFilter: excluded{}}),
		cache.SetRespectRBAC(cache.RespectRbacNormal),
		cache.SetPopulateResourceInfoHandler(func(un *unstructured.Unstructured, _ bool) (any, bool) {
			app := tracking.AppOf(un)
			// Only manifests of resources an application owns are kept in
			// memory; for the rest the cache holds references only.
			return &ResourceInfo{AppID: app}, app != ""
		}),
	}
	if len(c.Namespaces) > 0 {
		opts = append(opts, cache.SetNamespaces(c.Namespaces), cache.SetClusterResources(false))
	}
	return cfg, cache.NewClusterCache(cfg, opts...), nil
}

// evictLocked drops the least recently used cache nobody holds; false
// when every cache is in use.
func (m *Manager) evictLocked() bool {
	var victim uuid.UUID
	var oldest time.Time
	found := false
	for id, e := range m.entries {
		if e.users > 0 {
			continue
		}
		if !found || e.lastUsed.Before(oldest) {
			victim, oldest, found = id, e.lastUsed, true
		}
	}
	if found {
		m.dropLocked(victim)
	}
	return found
}

func (m *Manager) dropLocked(id uuid.UUID) {
	e := m.entries[id]
	if e == nil {
		return
	}
	delete(m.entries, id)
	e.stale = true
	if e.users <= 0 && e.cache != nil {
		e.cache.Invalidate()
	}
}

// Drop forgets a cluster's cache (the cluster changed or was deleted).
func (m *Manager) Drop(id uuid.UUID) {
	m.mu.Lock()
	defer m.mu.Unlock()
	m.dropLocked(id)
}

// DropWorkspace forgets every cache of a workspace.
func (m *Manager) DropWorkspace(workspace uuid.UUID) {
	m.mu.Lock()
	defer m.mu.Unlock()
	for id, e := range m.entries {
		if e.cluster.WorkspaceID == workspace {
			m.dropLocked(id)
		}
	}
}

// Len is how many caches exist.
func (m *Manager) Len() int {
	m.mu.Lock()
	defer m.mu.Unlock()
	return len(m.entries)
}

// Janitor drops caches unused for IdleTimeout until ctx ends, then all.
func (m *Manager) Janitor(ctx context.Context) {
	ticker := time.NewTicker(time.Minute)
	defer ticker.Stop()
	for {
		select {
		case <-ctx.Done():
			m.mu.Lock()
			for id := range m.entries {
				m.dropLocked(id)
			}
			m.mu.Unlock()
			return
		case <-ticker.C:
			m.expire(time.Now().Add(-IdleTimeout))
		}
	}
}

func (m *Manager) expire(before time.Time) {
	m.mu.Lock()
	defer m.mu.Unlock()
	for id, e := range m.entries {
		if e.users <= 0 && e.lastUsed.Before(before) {
			m.log.Debug("cluster cache idle; dropped", "cluster", id)
			m.dropLocked(id)
		}
	}
}

func secretError(name string, err error) string {
	return fmt.Sprintf("the kubeconfig secret %s could not be read: %s", name, status.Convert(err).Message())
}

// describe is a Kubernetes client error as one short line.
func describe(err error) string {
	msg := err.Error()
	if i := strings.IndexByte(msg, '\n'); i >= 0 {
		msg = msg[:i]
	}
	if len(msg) > 300 {
		msg = msg[:300] + "..."
	}
	return msg
}

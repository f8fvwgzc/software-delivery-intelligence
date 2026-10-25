// Package store is the service's Postgres access. Every function takes the
// workspace and filters on it, so a row of another workspace is never read
// or written, whatever id a caller passes (CONTRACT.md §18).
package store

import (
	"maps"
	"slices"
	"time"

	"github.com/google/uuid"
)

// Cluster is a Kubernetes cluster a workspace deploys to (§24.2).
type Cluster struct {
	ID               uuid.UUID
	WorkspaceID      uuid.UUID
	Name             string
	KubeconfigSecret string
	Context          string
	Server           string
	DefaultNamespace string
	Namespaces       []string
	Status           string
	Message          string
	ServerVersion    string
	CheckedAt        *time.Time
	CreatedAt        time.Time
	UpdatedAt        time.Time
	// ApplicationCount is filled by ListClusters.
	ApplicationCount int
}

// Fingerprint changes whenever anything that decides how the cluster is
// reached changes, so a cached connection built from older settings is
// never reused.
func (c Cluster) Fingerprint() string {
	return c.UpdatedAt.UTC().Format(time.RFC3339Nano) + "|" + c.KubeconfigSecret + "|" + c.Context
}

// Source is where the desired state comes from (Argo CD's spec.source).
type Source struct {
	Repo            string            `json:"repo"`
	Revision        string            `json:"revision"`
	Path            string            `json:"path"`
	Type            string            `json:"type"`
	Recurse         bool              `json:"recurse"`
	HelmValueFiles  []string          `json:"helm_value_files"`
	HelmParameters  map[string]string `json:"helm_parameters"`
	HelmReleaseName string            `json:"helm_release_name"`
	KustomizeImages []string          `json:"kustomize_images"`
}

// Clone is a deep copy.
func (s Source) Clone() Source {
	out := s
	out.HelmValueFiles = slices.Clone(s.HelmValueFiles)
	out.KustomizeImages = slices.Clone(s.KustomizeImages)
	out.HelmParameters = maps.Clone(s.HelmParameters)
	return out
}

// SyncPolicy is Argo CD's spec.syncPolicy, reduced to what SDI supports.
type SyncPolicy struct {
	Automated       bool   `json:"automated"`
	Prune           bool   `json:"prune"`
	SelfHeal        bool   `json:"self_heal"`
	CreateNamespace bool   `json:"create_namespace"`
	RetryLimit      uint32 `json:"retry_limit"`
}

// DefaultRetryLimit applies when RetryLimit is 0.
const DefaultRetryLimit = 3

// Retries is the effective number of retries of a failed automated sync.
func (p SyncPolicy) Retries() int {
	if p.RetryLimit == 0 {
		return DefaultRetryLimit
	}
	return int(p.RetryLimit)
}

// ResourceStatus is one resource of the application's tree.
type ResourceStatus struct {
	Group         string `json:"group"`
	Version       string `json:"version"`
	Kind          string `json:"kind"`
	Namespace     string `json:"namespace"`
	Name          string `json:"name"`
	SyncStatus    string `json:"sync_status"`
	HealthStatus  string `json:"health_status"`
	HealthMessage string `json:"health_message"`
	Hook          bool   `json:"hook"`
	SyncWave      int32  `json:"sync_wave"`
}

// Operation is the sync in flight or the last one that ran.
type Operation struct {
	Kind        string     `json:"kind"`
	Phase       string     `json:"phase"`
	Message     string     `json:"message"`
	Revision    string     `json:"revision"`
	InitiatedBy string     `json:"initiated_by"`
	ExecutionID string     `json:"execution_id"`
	StartedAt   time.Time  `json:"started_at"`
	FinishedAt  *time.Time `json:"finished_at,omitempty"`
}

// Application is one deployable unit: a source, a destination, a policy
// and what the last comparison found.
type Application struct {
	ID             uuid.UUID
	WorkspaceID    uuid.UUID
	ProjectID      string
	Name           string
	ClusterID      uuid.UUID
	ClusterName    string
	Namespace      string
	Source         Source
	Policy         SyncPolicy
	SyncStatus     string
	HealthStatus   string
	HealthMessage  string
	Revision       string
	DesiredDigest  string
	SyncedRevision string
	SyncedDigest   string
	Conditions     []string
	Resources      []ResourceStatus
	ResourceCount  int
	OutOfSyncCount int
	ReconciledAt   *time.Time
	Operation      *Operation
	OpLeaseUntil   *time.Time
	AutoDigest     string
	AutoAttempts   int
	AutoNextAt     *time.Time
	CreatedAt      time.Time
	UpdatedAt      time.Time
}

// OperationRunning is whether an unexpired lease holds the application.
func (a Application) OperationRunning(now time.Time) bool {
	return a.OpLeaseUntil != nil && a.OpLeaseUntil.After(now)
}

// HistoryEntry is one sync that was not a dry run.
type HistoryEntry struct {
	ID            uuid.UUID
	WorkspaceID   uuid.UUID
	ApplicationID uuid.UUID
	Kind          string
	Revision      string
	Source        Source
	Phase         string
	Message       string
	InitiatedBy   string
	ExecutionID   string
	Automated     bool
	StartedAt     time.Time
	FinishedAt    time.Time
}

// AppRef addresses an application for the reconcile loop.
type AppRef struct {
	ID          uuid.UUID
	WorkspaceID uuid.UUID
}

// Status is what one completed comparison writes.
type Status struct {
	SyncStatus     string
	HealthStatus   string
	HealthMessage  string
	Revision       string
	DesiredDigest  string
	Conditions     []string
	Resources      []ResourceStatus
	ResourceCount  int
	OutOfSyncCount int
	ReconciledAt   time.Time
}

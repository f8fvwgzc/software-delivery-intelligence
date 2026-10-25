// Package gitops is what Argo CD's application controller and repo-server
// do around the gitops engine (CONTRACT.md §24): render an application's
// desired state, compare it with the cluster, sync it, keep its history and
// reconcile it on a schedule, automating where its policy says so.
package gitops

import (
	"context"
	"log/slog"
	"time"

	"github.com/go-logr/logr"
	"github.com/google/uuid"

	"github.com/f8fvwgzc/software-delivery-intelligence/services/gitops/internal/clusters"
	"github.com/f8fvwgzc/software-delivery-intelligence/services/gitops/internal/events"
	"github.com/f8fvwgzc/software-delivery-intelligence/services/gitops/internal/github"
	"github.com/f8fvwgzc/software-delivery-intelligence/services/gitops/internal/peers"
	"github.com/f8fvwgzc/software-delivery-intelligence/services/gitops/internal/principal"
	"github.com/f8fvwgzc/software-delivery-intelligence/services/gitops/internal/render"
	"github.com/f8fvwgzc/software-delivery-intelligence/services/gitops/internal/store"
)

// ManagerName is the field manager SDI applies with.
const ManagerName = "sdi-gitops"

// ProjectSource reads knowledge projects.
type ProjectSource interface {
	GetProject(ctx context.Context, p principal.Principal, id string) (peers.Project, error)
}

// Auditor writes audit entries (fire-and-forget).
type Auditor interface {
	RecordAudit(p principal.Principal, action, entity, entityID, detail string)
}

// Repos reads source repositories.
type Repos interface {
	DefaultBranch(ctx context.Context, workspace uuid.UUID, repo string) (string, error)
	ResolveCommit(ctx context.Context, workspace uuid.UUID, repo, revision string) (string, error)
	Download(ctx context.Context, workspace uuid.UUID, repo, sha string) (*github.Checkout, error)
}

// Service is the controller.
type Service struct {
	Store       *store.Store
	Clusters    *clusters.Manager
	Repos       Repos
	Projects    ProjectSource
	Audit       Auditor
	Events      *events.Hub
	Log         *slog.Logger
	EngineLog   logr.Logger
	RenderCache *ManifestCache
	Limits      render.Limits
	// SyncWaveDelay is the pause between two sync waves, so controllers can
	// react to one wave before the next is applied (Argo CD: 2 s).
	SyncWaveDelay time.Duration
	// WaveObserver, when set, hears every completed sync wave (tests).
	WaveObserver func(app uuid.UUID, phase string, wave int)
	// Shutdown ends with the process: running syncs are terminated when it
	// does.
	Shutdown context.Context

	renderSlots chan struct{}
	refreshes   chan struct{}
}

// Options configure New.
type Options struct {
	Store         *store.Store
	Clusters      *clusters.Manager
	Repos         Repos
	Projects      ProjectSource
	Audit         Auditor
	Events        *events.Hub
	Log           *slog.Logger
	EngineLog     logr.Logger
	Shutdown      context.Context
	SyncWaveDelay time.Duration
}

const (
	// renderConcurrency bounds downloads and renders at once (disk and CPU).
	renderConcurrency = 4
	// refreshConcurrency bounds background refreshes started by RPCs.
	refreshConcurrency = 8
)

// New builds the controller.
func New(o Options) *Service {
	shutdown := o.Shutdown
	if shutdown == nil {
		shutdown = context.Background()
	}
	return &Service{
		Store:         o.Store,
		Clusters:      o.Clusters,
		Repos:         o.Repos,
		Projects:      o.Projects,
		Audit:         o.Audit,
		Events:        o.Events,
		Log:           o.Log,
		EngineLog:     o.EngineLog,
		RenderCache:   NewManifestCache(ManifestTTL, ManifestMaxItems, ManifestMaxBytes),
		Limits:        render.DefaultLimits,
		SyncWaveDelay: o.SyncWaveDelay,
		Shutdown:      shutdown,
		renderSlots:   make(chan struct{}, renderConcurrency),
		refreshes:     make(chan struct{}, refreshConcurrency),
	}
}

// RefreshInBackground compares an application soon, without blocking the
// caller (after an upsert); skipped when enough refreshes already run.
func (s *Service) RefreshInBackground(workspace, id uuid.UUID) {
	select {
	case s.refreshes <- struct{}{}:
	default:
		return
	}
	go func() {
		defer func() { <-s.refreshes }()
		ctx, cancel := context.WithTimeout(s.Shutdown, 5*time.Minute)
		defer cancel()
		app, err := s.Store.GetApplication(ctx, workspace, id)
		if err != nil {
			return
		}
		if _, err := s.Refresh(ctx, app, false); err != nil {
			s.Log.Debug("background refresh failed", "application", id, "error", err)
		}
	}()
}

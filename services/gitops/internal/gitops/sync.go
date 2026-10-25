package gitops

import (
	"context"
	"errors"
	"fmt"
	"sort"
	"strings"
	"time"

	"github.com/google/uuid"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/status"
	metav1 "k8s.io/apimachinery/pkg/apis/meta/v1"
	"k8s.io/apimachinery/pkg/apis/meta/v1/unstructured"

	"github.com/argoproj/argo-cd/gitops-engine/pkg/cache"
	enginesync "github.com/argoproj/argo-cd/gitops-engine/pkg/sync"
	"github.com/argoproj/argo-cd/gitops-engine/pkg/sync/common"
	"github.com/argoproj/argo-cd/gitops-engine/pkg/utils/kube"

	"github.com/f8fvwgzc/software-delivery-intelligence/services/gitops/internal/clusters"
	"github.com/f8fvwgzc/software-delivery-intelligence/services/gitops/internal/principal"
	"github.com/f8fvwgzc/software-delivery-intelligence/services/gitops/internal/render"
	"github.com/f8fvwgzc/software-delivery-intelligence/services/gitops/internal/store"
	"github.com/f8fvwgzc/software-delivery-intelligence/services/gitops/internal/tracking"
)

// Operation kinds and phases (CONTRACT.md §24.5).
const (
	KindSync     = "sync"
	KindRollback = "rollback"

	PhaseRunning   = "running"
	PhaseSucceeded = "succeeded"
	PhaseFailed    = "failed"
	PhaseError     = "error"

	DefaultSyncTimeout = 600 * time.Second
	MaxSyncTimeout     = 3600 * time.Second

	// operationLease is how long a claim outlives a replica that died
	// mid-operation; a running operation renews it.
	operationLease = 2 * time.Minute
	leaseRenewal   = 30 * time.Second
	// terminateGrace is how long a terminated operation may take to wind
	// down its hooks.
	terminateGrace = 30 * time.Second
	// settleTimeout bounds the wait for the cache to see what a sync applied
	// before the post-sync comparison.
	settleTimeout = 10 * time.Second
)

// ResourceRef names one resource of a partial sync.
type ResourceRef struct {
	Group, Kind, Namespace, Name string
}

// SyncRequest is one sync or rollback.
type SyncRequest struct {
	Kind string
	// Revision overrides the source's revision.
	Revision string
	// Source overrides the application's source (a rollback's history entry).
	Source          *store.Source
	Prune           bool
	DryRun          bool
	Resources       []ResourceRef
	KustomizeImages []string
	HelmParameters  map[string]string
	ExecutionID     string
	Timeout         time.Duration
	Automated       bool
	Principal       principal.Principal
}

// ResourceResult is what a sync did to one resource.
type ResourceResult struct {
	Group, Kind, Namespace, Name string
	// "synced" | "pruned" | "prune_skipped" | "failed" | "unchanged"
	Status  string
	Message string
	// "pre_sync" | "sync" | "post_sync" | "sync_fail" | ""
	HookPhase string
	SyncWave  int32
}

// SyncOutcome is the result of an operation.
type SyncOutcome struct {
	ApplicationID uuid.UUID
	Phase         string
	Message       string
	Revision      string
	Resources     []ResourceResult
	HistoryID     string
	HealthStatus  string
	StartedAt     time.Time
	FinishedAt    time.Time
	// Digest is the desired state that was synced ("" when none rendered).
	Digest string
}

// ErrOperationRunning is a second operation on one application.
var ErrOperationRunning = status.Error(codes.FailedPrecondition, "another operation is running on this application; wait for it to finish")

// MergeOverrides stores image overrides and helm parameters into a source,
// merged by key (image name, parameter name): the desired state stays
// declared.
func MergeOverrides(src store.Source, images []string, params map[string]string) (store.Source, error) {
	out := src.Clone()
	for _, raw := range images {
		img, err := render.ParseImage(raw)
		if err != nil {
			return src, err
		}
		replaced := false
		for i, existing := range out.KustomizeImages {
			if prev, err := render.ParseImage(existing); err == nil && prev.Name == img.Name {
				out.KustomizeImages[i] = strings.TrimSpace(raw)
				replaced = true
				break
			}
		}
		if !replaced {
			out.KustomizeImages = append(out.KustomizeImages, strings.TrimSpace(raw))
		}
	}
	if len(params) > 0 && out.HelmParameters == nil {
		out.HelmParameters = map[string]string{}
	}
	for k, v := range params {
		if strings.TrimSpace(k) == "" {
			return src, fmt.Errorf("a helm parameter has an empty name")
		}
		out.HelmParameters[k] = v
	}
	return out, nil
}

// Sync applies the desired state and waits for the operation to end
// (CONTRACT.md §24.5). It returns an error only when the operation cannot
// be claimed or the request is invalid; a sync that fails to render, to
// reach the cluster or to apply is an outcome with phase "error" or
// "failed", recorded in history like any other.
func (s *Service) Sync(ctx context.Context, app store.Application, req SyncRequest) (*SyncOutcome, error) {
	if req.Kind == "" {
		req.Kind = KindSync
	}
	if req.Timeout <= 0 {
		req.Timeout = DefaultSyncTimeout
	}
	if req.Timeout > MaxSyncTimeout {
		req.Timeout = MaxSyncTimeout
	}
	source := app.Source
	if req.Source != nil {
		source = req.Source.Clone()
	}
	if len(req.KustomizeImages) > 0 || len(req.HelmParameters) > 0 {
		merged, err := MergeOverrides(source, req.KustomizeImages, req.HelmParameters)
		if err != nil {
			return nil, status.Error(codes.InvalidArgument, err.Error())
		}
		source = merged
	}
	// The work outlives the caller's connection, bounded by the timeout: an
	// operation that started is always finished and recorded.
	work := context.WithoutCancel(ctx)
	started := time.Now().UTC()
	op := store.Operation{
		Kind: req.Kind, Phase: PhaseRunning, Revision: req.Revision,
		InitiatedBy: req.Principal.UserID, ExecutionID: req.ExecutionID, StartedAt: started,
	}
	claimed, err := s.Store.ClaimOperation(work, app.WorkspaceID, app.ID, op, operationLease)
	if err != nil {
		return nil, err
	}
	if !claimed {
		return nil, ErrOperationRunning
	}
	renewCtx, stopRenew := context.WithCancel(work)
	go s.renewLease(renewCtx, app)
	s.Events.Application(app.WorkspaceID, app.ID, app.ProjectID, "progress", PhaseRunning,
		map[string]string{"operation": req.Kind, "name": app.Name})

	if !req.DryRun && req.Source == nil && (len(req.KustomizeImages) > 0 || len(req.HelmParameters) > 0) {
		if err := s.Store.UpdateSource(work, app.WorkspaceID, app.ID, source); err != nil {
			stopRenew()
			s.finish(work, app, req, op, &SyncOutcome{ApplicationID: app.ID, Phase: PhaseError, Message: "the source could not be stored", StartedAt: started, FinishedAt: time.Now().UTC()})
			return nil, err
		}
		app.Source = source
	}

	out := s.run(work, app, source, req, started)
	stopRenew()
	s.finish(work, app, req, op, out)
	return out, nil
}

func (s *Service) renewLease(ctx context.Context, app store.Application) {
	ticker := time.NewTicker(leaseRenewal)
	defer ticker.Stop()
	for {
		select {
		case <-ctx.Done():
			return
		case <-ticker.C:
			if err := s.Store.RenewLease(ctx, app.WorkspaceID, app.ID, operationLease); err != nil && ctx.Err() == nil {
				s.Log.Warn("could not renew the operation lease", "application", app.ID, "error", err)
			}
		}
	}
}

// run performs the operation and never fails: every problem is an outcome.
func (s *Service) run(ctx context.Context, app store.Application, source store.Source, req SyncRequest, started time.Time) *SyncOutcome {
	out := &SyncOutcome{ApplicationID: app.ID, StartedAt: started, Revision: req.Revision}
	fail := func(phase, msg string) *SyncOutcome {
		out.Phase, out.Message, out.FinishedAt = phase, msg, time.Now().UTC()
		return out
	}
	cluster, err := s.Store.GetCluster(ctx, app.WorkspaceID, app.ClusterID)
	if err != nil {
		return fail(PhaseError, "the application's cluster could not be read")
	}
	h, err := s.Clusters.Acquire(ctx, cluster)
	if err != nil {
		return fail(PhaseError, err.Error())
	}
	defer h.Release()
	d, err := s.Desired(ctx, app, source, req.Revision, h, false)
	if err != nil {
		return fail(PhaseError, err.Error())
	}
	out.Revision, out.Digest = d.Revision, d.Digest
	cmp, err := s.compare(app, h, d)
	if err != nil {
		return fail(PhaseError, err.Error())
	}

	phase, message, results, err := s.apply(ctx, app, h, cmp, req)
	if err != nil {
		return fail(PhaseError, err.Error())
	}
	out.Resources = results
	out.Phase, out.Message = phase, message
	// Desired resources that exist without this application's tracking
	// annotation were left alone; the sync cannot be called a success.
	var skipped []ResourceResult
	for key, why := range cmp.Foreign {
		if !selected(req.Resources, key) {
			continue
		}
		skipped = append(skipped, ResourceResult{Group: key.Group, Kind: key.Kind, Namespace: key.Namespace, Name: key.Name,
			Status: "failed", Message: why, SyncWave: waveOf(cmp, key)})
	}
	sortResults(skipped)
	out.Resources = append(out.Resources, skipped...)
	if len(skipped) > 0 && out.Phase == PhaseSucceeded {
		out.Phase = PhaseFailed
		out.Message = fmt.Sprintf("%d resource(s) exist in the cluster without this application's tracking annotation and were left alone", len(skipped))
	}
	out.FinishedAt = time.Now().UTC()

	// The application's health right after the operation.
	after := cmp
	if !req.DryRun {
		if out.Phase == PhaseSucceeded {
			after = s.settle(ctx, app, h, d)
		} else if again, err := s.compare(app, h, d); err == nil {
			after = again
		}
	}
	if after != nil {
		out.HealthStatus = after.Status.HealthStatus
		if !req.DryRun {
			if err := s.saveStatus(ctx, app, after.Status); err != nil {
				s.Log.Warn("could not record the comparison after a sync", "application", app.ID, "error", err)
			}
		}
	} else {
		out.HealthStatus = app.HealthStatus
	}
	return out
}

// settle compares again once the cache reflects what was applied (or a
// few seconds passed), so the answer carries the health right after.
func (s *Service) settle(ctx context.Context, app store.Application, h *clusters.Handle, d *Desired) *Comparison {
	deadline := time.Now().Add(settleTimeout)
	var last *Comparison
	for {
		cmp, err := s.compare(app, h, d)
		if err == nil {
			last = cmp
			if cmp.Status.SyncStatus == SyncSynced || len(cmp.Foreign) > 0 {
				return cmp
			}
		}
		if time.Now().After(deadline) {
			return last
		}
		select {
		case <-ctx.Done():
			return last
		case <-time.After(500 * time.Millisecond):
		}
	}
}

// apply runs the engine's sync: phases PreSync, Sync, PostSync (SyncFail on
// failure), waves, kinds in dependency order, prune and dry run as asked.
func (s *Service) apply(ctx context.Context, app store.Application, h *clusters.Handle, cmp *Comparison, req SyncRequest) (string, string, []ResourceResult, error) {
	appID := app.ID.String()
	foreground := metav1.DeletePropagationForeground
	filter := func(key kube.ResourceKey, target, live *unstructured.Unstructured) bool {
		if _, ok := cmp.Foreign[key]; ok {
			return false
		}
		// Belt and braces: nothing without this application's own
		// tracking id is ever pruned or changed.
		if live != nil && !tracking.Owns(appID, live) {
			return false
		}
		return selected(req.Resources, key)
	}
	opts := []enginesync.SyncOpt{
		enginesync.WithLogr(s.EngineLog),
		// Partial syncs skip hooks (Argo CD: hooks run with the whole app).
		enginesync.WithOperationSettings(req.DryRun, req.Prune, false, len(req.Resources) > 0),
		enginesync.WithResourcesFilter(filter),
		enginesync.WithManifestValidation(true),
		enginesync.WithPrunePropagationPolicy(&foreground),
		enginesync.WithServerSideApplyManager(ManagerName),
		enginesync.WithSyncWaveHook(func(phase common.SyncPhase, wave int, final bool) error {
			if s.WaveObserver != nil {
				s.WaveObserver(app.ID, string(phase), wave)
			}
			s.Log.Debug("sync wave applied", "application", app.ID, "phase", string(phase), "wave", wave, "final", final)
			if !final && s.SyncWaveDelay > 0 {
				time.Sleep(s.SyncWaveDelay)
			}
			return nil
		}),
	}
	// A dry run must change nothing, so it never creates the namespace.
	if app.Policy.CreateNamespace && !req.DryRun {
		opts = append(opts, enginesync.WithNamespaceModifier(func(managed, live *unstructured.Unstructured) (bool, error) {
			return managed != nil && live == nil, nil
		}))
	}
	wake := make(chan struct{}, 1)
	targets := map[kube.ResourceKey]bool{}
	for _, t := range cmp.Desired.Objects {
		targets[kube.GetResourceKey(t)] = true
	}
	unsubscribe := h.Cache().OnResourceUpdated(func(newRes, oldRes *cache.Resource, _ map[kube.ResourceKey]*cache.Resource) {
		r := newRes
		if r == nil {
			r = oldRes
		}
		if r == nil || (clusters.AppOf(r) != appID && !targets[r.ResourceKey()]) {
			return
		}
		select {
		case wake <- struct{}{}:
		default:
		}
	})
	defer unsubscribe()

	// As Argo CD's controller does, every step builds the sync context
	// afresh from the live state (a hook that completed, a Deployment that
	// became healthy) and the operation state the previous step left.
	var (
		phase     common.OperationPhase
		message   string
		results   []common.ResourceSyncResult
		startedAt = metav1.NewTime(time.Now())
		rec       = cmp.Reconciliation
	)
	step := func(terminate bool) error {
		stepOpts := append(append([]enginesync.SyncOpt{}, opts...), enginesync.WithInitialState(phase, message, results, startedAt))
		syncCtx, cleanup, err := enginesync.NewSyncContext(cmp.Desired.Revision, rec, h.Config(), h.Config(),
			s.Clusters.Kubectl(), app.Namespace, stepOpts...)
		if err != nil {
			return fmt.Errorf("the sync could not start: %s", oneLine(err.Error()))
		}
		defer cleanup()
		if terminate {
			syncCtx.Terminate()
		} else {
			syncCtx.Sync()
		}
		phase, message, results = syncCtx.GetState()
		return nil
	}
	refresh := func() error {
		live, err := h.Cache().GetManagedLiveObjs(cmp.Desired.Objects, func(r *cache.Resource) bool {
			return clusters.AppOf(r) == appID
		})
		if err != nil {
			return fmt.Errorf("the live state could not be read: %s", oneLine(err.Error()))
		}
		rec = enginesync.Reconcile(cmp.Desired.Objects, live, app.Namespace, h.Cache())
		return nil
	}

	deadline := time.Now().Add(req.Timeout)
	var terminatedAt time.Time
	for {
		if err := step(false); err != nil {
			return "", "", nil, err
		}
		if phase.Completed() {
			return phaseName(phase), message, s.results(cmp, results), nil
		}
		now := time.Now()
		if terminatedAt.IsZero() && (now.After(deadline) || s.Shutdown.Err() != nil) {
			terminatedAt = now
			if err := refresh(); err == nil {
				_ = step(true)
			}
			if phase.Completed() {
				reason := fmt.Sprintf("the operation did not finish within %s and was terminated", req.Timeout)
				if s.Shutdown.Err() != nil {
					reason = "the service shut down and terminated the operation"
				}
				return phaseName(phase), reason + " (" + message + ")", s.results(cmp, results), nil
			}
		}
		if !terminatedAt.IsZero() && now.After(terminatedAt.Add(terminateGrace)) {
			return PhaseError, "the operation was terminated but did not wind down: " + message, s.results(cmp, results), nil
		}
		select {
		case <-wake:
		case <-time.After(time.Second):
		case <-ctx.Done():
			return PhaseError, "the operation was cancelled", s.results(cmp, results), nil
		}
		if err := refresh(); err != nil {
			s.Log.Warn("sync: live state unavailable; retrying", "application", app.ID, "error", err)
		}
	}
}

func phaseName(p common.OperationPhase) string {
	switch p {
	case common.OperationSucceeded:
		return PhaseSucceeded
	case common.OperationFailed:
		return PhaseFailed
	case common.OperationError:
		return PhaseError
	case common.OperationTerminating:
		return "terminating"
	}
	return PhaseRunning
}

// results maps the engine's resource results onto the contract's.
func (s *Service) results(cmp *Comparison, in []common.ResourceSyncResult) []ResourceResult {
	out := make([]ResourceResult, 0, len(in))
	for _, r := range in {
		res := ResourceResult{
			Group: r.ResourceKey.Group, Kind: r.ResourceKey.Kind, Namespace: r.ResourceKey.Namespace, Name: r.ResourceKey.Name,
			Message: oneLine(r.Message), SyncWave: waveOf(cmp, r.ResourceKey),
		}
		switch r.Status {
		case common.ResultCodeSynced:
			res.Status = "synced"
			if strings.HasSuffix(strings.TrimSuffix(strings.TrimSpace(r.Message), " (dry run)"), " unchanged") {
				res.Status = "unchanged"
			}
		case common.ResultCodePruned:
			res.Status = "pruned"
		case common.ResultCodePruneSkipped:
			res.Status = "prune_skipped"
		case common.ResultCodeSyncFailed:
			res.Status = "failed"
		default:
			res.Status = "failed"
		}
		// A hook (or a resource) that did not succeed is a failure even when
		// its manifest applied.
		if r.HookPhase == common.OperationFailed || r.HookPhase == common.OperationError {
			res.Status = "failed"
		}
		if r.HookType != "" {
			res.HookPhase = hookPhaseName(r.SyncPhase)
		}
		out = append(out, res)
	}
	return out
}

func hookPhaseName(p common.SyncPhase) string {
	switch p {
	case common.SyncPhasePreSync:
		return "pre_sync"
	case common.SyncPhaseSync:
		return "sync"
	case common.SyncPhasePostSync:
		return "post_sync"
	case common.SyncPhaseSyncFail:
		return "sync_fail"
	}
	return ""
}

// waveOf is a resource's sync wave. The engine reports cluster-scoped
// resources under the destination namespace, so the namespace only decides
// when it matches.
func waveOf(cmp *Comparison, key kube.ResourceKey) int32 {
	if w, ok := cmp.waves[key]; ok {
		return w
	}
	return cmp.wavesByName[kube.ResourceKey{Group: key.Group, Kind: key.Kind, Name: key.Name}]
}

// selected is whether a partial sync includes key (every resource when
// refs is empty).
func selected(refs []ResourceRef, key kube.ResourceKey) bool {
	if len(refs) == 0 {
		return true
	}
	for _, r := range refs {
		if r.Kind == key.Kind && r.Group == key.Group && r.Name == key.Name && (r.Namespace == "" || r.Namespace == key.Namespace) {
			return true
		}
	}
	return false
}

func sortResults(rs []ResourceResult) {
	sort.SliceStable(rs, func(i, j int) bool {
		a, b := rs[i], rs[j]
		if a.Kind != b.Kind {
			return a.Kind < b.Kind
		}
		if a.Namespace != b.Namespace {
			return a.Namespace < b.Namespace
		}
		return a.Name < b.Name
	})
}

// finish records the outcome: the operation and its lease, history (not
// for a dry run), automation bookkeeping, audit and the event.
func (s *Service) finish(ctx context.Context, app store.Application, req SyncRequest, op store.Operation, out *SyncOutcome) {
	finished := out.FinishedAt
	op.Phase, op.Message, op.Revision, op.FinishedAt = out.Phase, out.Message, out.Revision, &finished
	recorded := &op
	if req.DryRun {
		// A dry run leaves the last real operation in place.
		recorded = app.Operation
	}
	var synced *[2]string
	if !req.DryRun && out.Phase == PhaseSucceeded && out.Digest != "" {
		synced = &[2]string{out.Revision, out.Digest}
	}
	if err := s.Store.FinishOperation(ctx, app.WorkspaceID, app.ID, recorded, synced); err != nil {
		s.Log.Error("could not record the end of an operation", "application", app.ID, "error", err)
	}
	if synced != nil && !req.Automated {
		// A sync that succeeded by hand ends any automated retries.
		if err := s.Store.SaveAutomation(ctx, app.WorkspaceID, app.ID, "", 0, nil); err != nil {
			s.Log.Warn("could not reset automation state", "application", app.ID, "error", err)
		}
	}
	if req.DryRun {
		return
	}
	entry := store.HistoryEntry{
		ID: uuid.Must(uuid.NewV7()), WorkspaceID: app.WorkspaceID, ApplicationID: app.ID,
		Kind: req.Kind, Revision: out.Revision, Phase: out.Phase, Message: out.Message,
		InitiatedBy: req.Principal.UserID, ExecutionID: req.ExecutionID, Automated: req.Automated,
		StartedAt: out.StartedAt, FinishedAt: out.FinishedAt,
	}
	entry.Source = app.Source
	if req.Source != nil {
		entry.Source = *req.Source
	}
	if out.Revision != "" {
		// The source as synced: pinned to the commit, so a rollback syncs
		// exactly this state again.
		entry.Source.Revision = out.Revision
	}
	if err := s.Store.InsertHistory(ctx, entry); err != nil {
		s.Log.Error("could not record history", "application", app.ID, "error", err)
	} else {
		out.HistoryID = entry.ID.String()
	}
	action := "application.sync"
	switch {
	case req.Automated:
		action = "application.auto_sync"
	case req.Kind == KindRollback:
		action = "application.rollback"
	}
	detail := fmt.Sprintf("%s at %s: %s", req.Kind, short(out.Revision), out.Phase)
	if req.ExecutionID != "" {
		detail += "; execution " + req.ExecutionID
	}
	s.Audit.RecordAudit(req.Principal, action, "application", app.ID.String(), detail)
	s.Events.Application(app.WorkspaceID, app.ID, app.ProjectID, "updated", out.Phase, map[string]string{
		"operation": req.Kind, "phase": out.Phase, "revision": out.Revision, "name": app.Name,
		"health_status": out.HealthStatus,
	})
}

func short(sha string) string {
	if len(sha) > 7 {
		return sha[:7]
	}
	if sha == "" {
		return "no revision"
	}
	return sha
}

// IsSourceError is a render or repository failure.
func IsSourceError(err error) bool {
	var se *SourceError
	return errors.As(err, &se)
}

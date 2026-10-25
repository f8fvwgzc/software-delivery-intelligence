package gitops

import (
	"context"
	"sync"
	"time"

	"github.com/google/uuid"
	"github.com/jackc/pgx/v5/pgxpool"

	"github.com/f8fvwgzc/software-delivery-intelligence/services/gitops/internal/principal"
	"github.com/f8fvwgzc/software-delivery-intelligence/services/gitops/internal/store"
)

const (
	// ReconcileConcurrency is how many applications a round handles at once.
	ReconcileConcurrency = 8
	// appBudget bounds one application's turn (a compare, maybe a sync).
	appBudget = DefaultSyncTimeout + 5*time.Minute
)

// leaderLock is the one-replica-at-a-time lock of the loop, per schema.
const leaderLock = `SELECT pg_try_advisory_lock(hashtextextended('sdi.gitops.reconcile:' || current_schema(), 0))`
const leaderUnlock = `SELECT pg_advisory_unlock(hashtextextended('sdi.gitops.reconcile:' || current_schema(), 0))`

// Reconciler compares every application on a schedule and runs automated
// syncs (CONTRACT.md §24.6).
type Reconciler struct {
	svc      *Service
	interval time.Duration

	mu      sync.Mutex
	pending map[store.AppRef]bool
	kick    chan struct{}
}

// NewReconciler builds the loop; interval 0 turns it off.
func NewReconciler(svc *Service, interval time.Duration) *Reconciler {
	return &Reconciler{svc: svc, interval: interval, pending: map[store.AppRef]bool{}, kick: make(chan struct{}, 1)}
}

// Enqueue asks for one application to be handled soon (a retry whose
// backoff ended), outside the regular schedule.
func (r *Reconciler) Enqueue(ref store.AppRef) {
	r.mu.Lock()
	r.pending[ref] = true
	r.mu.Unlock()
	select {
	case r.kick <- struct{}{}:
	default:
	}
}

// Run loops until ctx ends.
func (r *Reconciler) Run(ctx context.Context) {
	if r.interval <= 0 {
		r.svc.Log.Info("reconcile loop off (GITOPS_RECONCILE_SECONDS=0)")
		return
	}
	ticker := time.NewTicker(r.interval)
	defer ticker.Stop()
	// A first round shortly after start, then on the schedule.
	first := time.NewTimer(10 * time.Second)
	defer first.Stop()
	for {
		select {
		case <-ctx.Done():
			return
		case <-first.C:
			r.round(ctx, nil)
		case <-ticker.C:
			r.round(ctx, nil)
		case <-r.kick:
			r.mu.Lock()
			refs := make([]store.AppRef, 0, len(r.pending))
			for ref := range r.pending {
				refs = append(refs, ref)
			}
			r.pending = map[store.AppRef]bool{}
			r.mu.Unlock()
			r.round(ctx, refs)
		}
	}
}

// round handles refs (every application when nil) while holding the
// leadership lock; another replica holding it means this one skips.
func (r *Reconciler) round(ctx context.Context, refs []store.AppRef) {
	pool := r.svc.Store.Pool()
	conn, ok := tryLead(ctx, pool)
	if !ok {
		return
	}
	defer func() {
		_, _ = conn.Exec(context.WithoutCancel(ctx), leaderUnlock)
		conn.Release()
	}()
	if refs == nil {
		var err error
		refs, err = r.svc.Store.AllApplications(ctx)
		if err != nil {
			r.svc.Log.Warn("reconcile: could not list applications", "error", err)
			return
		}
	}
	sem := make(chan struct{}, ReconcileConcurrency)
	var wg sync.WaitGroup
	for _, ref := range refs {
		select {
		case <-ctx.Done():
			wg.Wait()
			return
		case sem <- struct{}{}:
		}
		wg.Add(1)
		go func(ref store.AppRef) {
			defer wg.Done()
			defer func() { <-sem }()
			appCtx, cancel := context.WithTimeout(ctx, appBudget)
			defer cancel()
			r.ReconcileOne(appCtx, ref)
		}(ref)
	}
	wg.Wait()
}

func tryLead(ctx context.Context, pool *pgxpool.Pool) (*pgxpool.Conn, bool) {
	conn, err := pool.Acquire(ctx)
	if err != nil {
		return nil, false
	}
	var leader bool
	if err := conn.QueryRow(ctx, leaderLock).Scan(&leader); err != nil || !leader {
		conn.Release()
		return nil, false
	}
	return conn, true
}

// ReconcileOne compares one application and, when its policy is automated
// and the decision says so, syncs it as the system principal.
func (r *Reconciler) ReconcileOne(ctx context.Context, ref store.AppRef) {
	svc := r.svc
	app, err := svc.Store.GetApplication(ctx, ref.WorkspaceID, ref.ID)
	if err != nil {
		return
	}
	before := app
	app, err = svc.Refresh(ctx, app, false)
	if err != nil {
		svc.Log.Warn("reconcile: comparison could not be recorded", "application", ref.ID, "error", err)
		return
	}
	if !app.Policy.Automated || app.ReconciledAt == nil || (before.ReconciledAt != nil && !app.ReconciledAt.After(*before.ReconciledAt)) {
		// Not automated, or the comparison did not complete: nothing to act on.
		return
	}
	in := AutoInput{
		Policy: app.Policy,
		Status: store.Status{
			SyncStatus: app.SyncStatus, HealthStatus: app.HealthStatus, Revision: app.Revision,
			DesiredDigest: app.DesiredDigest, Resources: app.Resources,
		},
		SyncedDigest:     app.SyncedDigest,
		AutoDigest:       app.AutoDigest,
		AutoAttempts:     app.AutoAttempts,
		AutoNextAt:       app.AutoNextAt,
		OperationRunning: app.OperationRunning(time.Now()),
		Now:              time.Now(),
	}
	decision := Decide(in)
	if decision.Condition != "" {
		conditions := append(append([]string{}, app.Conditions...), decision.Condition)
		if err := svc.Store.SaveConditions(ctx, app.WorkspaceID, app.ID, conditions); err != nil {
			svc.Log.Warn("reconcile: could not record a condition", "application", app.ID, "error", err)
		}
	}
	if decision.RetryAt != nil {
		r.retryAt(ref, *decision.RetryAt)
	}
	if !decision.Sync {
		svc.Log.Debug("reconcile: no automated sync", "application", app.ID, "reason", decision.Reason)
		return
	}
	req := SyncRequest{
		Kind:      KindSync,
		Prune:     app.Policy.Prune,
		Automated: true,
		Principal: principal.System(app.WorkspaceID),
	}
	if decision.SelfHeal {
		// Self-heal syncs only what drifted, without hooks.
		for _, res := range app.Resources {
			if !res.Hook && res.SyncStatus != SyncSynced {
				req.Resources = append(req.Resources, ResourceRef{Group: res.Group, Kind: res.Kind, Namespace: res.Namespace, Name: res.Name})
			}
		}
	}
	svc.Log.Info("automated sync", "application", app.ID, "revision", short(app.Revision), "self_heal", decision.SelfHeal)
	out, err := svc.Sync(ctx, app, req)
	if err != nil {
		svc.Log.Debug("automated sync did not start", "application", app.ID, "error", err)
		return
	}
	in.Status.DesiredDigest = app.DesiredDigest
	if out.Digest != "" {
		in.Status.DesiredDigest = out.Digest
	}
	// The backoff runs from the end of the failed attempt.
	in.Now = time.Now()
	digest, attempts, next := AfterAttempt(in, out.Phase == PhaseSucceeded)
	if err := svc.Store.SaveAutomation(ctx, app.WorkspaceID, app.ID, digest, attempts, next); err != nil {
		svc.Log.Warn("reconcile: could not record automation state", "application", app.ID, "error", err)
	}
	if next != nil {
		svc.Log.Info("automated sync failed; retrying later", "application", app.ID, "attempts", attempts, "retry_at", next.Format(time.RFC3339))
		r.retryAt(ref, *next)
	}
}

// retryAt enqueues ref once at is reached.
func (r *Reconciler) retryAt(ref store.AppRef, at time.Time) {
	wait := time.Until(at)
	if wait < 0 {
		wait = 0
	}
	time.AfterFunc(wait+100*time.Millisecond, func() { r.Enqueue(ref) })
}

// ReconcileAll runs one round now, for every application (tests, and an
// operator's manual trigger).
func (r *Reconciler) ReconcileAll(ctx context.Context) {
	r.round(ctx, nil)
}

// Ref addresses an application.
func Ref(workspace, id uuid.UUID) store.AppRef {
	return store.AppRef{ID: id, WorkspaceID: workspace}
}

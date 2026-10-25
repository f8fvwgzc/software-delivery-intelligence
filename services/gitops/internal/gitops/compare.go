package gitops

import (
	"context"
	"fmt"
	"sort"
	"strings"
	"time"

	"k8s.io/apimachinery/pkg/apis/meta/v1/unstructured"

	"github.com/argoproj/argo-cd/gitops-engine/pkg/cache"
	"github.com/argoproj/argo-cd/gitops-engine/pkg/diff"
	"github.com/argoproj/argo-cd/gitops-engine/pkg/health"
	hookutil "github.com/argoproj/argo-cd/gitops-engine/pkg/sync/hook"
	"github.com/argoproj/argo-cd/gitops-engine/pkg/sync/ignore"
	"github.com/argoproj/argo-cd/gitops-engine/pkg/utils/kube"

	enginesync "github.com/argoproj/argo-cd/gitops-engine/pkg/sync"

	"github.com/f8fvwgzc/software-delivery-intelligence/services/gitops/internal/clusters"
	"github.com/f8fvwgzc/software-delivery-intelligence/services/gitops/internal/store"
	"github.com/f8fvwgzc/software-delivery-intelligence/services/gitops/internal/tracking"
)

// Sync and health vocabularies (CONTRACT.md §24.4).
const (
	SyncSynced     = "synced"
	SyncOutOfSync  = "out_of_sync"
	SyncMissing    = "missing"
	SyncExtraneous = "extraneous"
	SyncUnknown    = "unknown"

	HealthHealthy     = "healthy"
	HealthProgressing = "progressing"
	HealthDegraded    = "degraded"
	HealthSuspended   = "suspended"
	HealthMissing     = "missing"
	HealthUnknown     = "unknown"
)

// Comparison is the desired state set against the cluster.
type Comparison struct {
	Desired        *Desired
	Reconciliation enginesync.ReconciliationResult
	// Diffs line up with Reconciliation.Target / .Live.
	Diffs []diff.DiffResult
	// Foreign are live resources the desired state names that this
	// application did not create: never applied, never pruned.
	Foreign map[kube.ResourceKey]string
	Status  store.Status

	waves       map[kube.ResourceKey]int32
	wavesByName map[kube.ResourceKey]int32
}

// statusNormalizer is Argo CD's default ignore rule (ignoreResourceStatusField:
// all): the status field is the cluster's, never part of the desired state.
type statusNormalizer struct{}

func (statusNormalizer) Normalize(un *unstructured.Unstructured) error {
	if un != nil {
		unstructured.RemoveNestedField(un.Object, "status")
	}
	return nil
}

// diffOptions are the engine's diff settings, with Argo CD's normalisation.
func (s *Service) diffOptions(c cache.ClusterCache) []diff.Option {
	return []diff.Option{
		diff.WithNormalizer(statusNormalizer{}),
		diff.IgnoreAggregatedRoles(false),
		diff.WithGVKParser(c.GetGVKParser()),
		diff.WithManager(ManagerName),
		diff.WithLogr(s.EngineLog),
	}
}

// compare takes the managed live objects from the cluster cache and diffs
// each desired/live pair with the engine.
func (s *Service) compare(app store.Application, h *clusters.Handle, d *Desired) (*Comparison, error) {
	appID := app.ID.String()
	live, err := h.Cache().GetManagedLiveObjs(d.Objects, func(r *cache.Resource) bool {
		return clusters.AppOf(r) == appID
	})
	if err != nil {
		return nil, fmt.Errorf("the live state could not be read: %s", oneLine(err.Error()))
	}
	rec := enginesync.Reconcile(d.Objects, live, app.Namespace, h.Cache())
	foreign := map[kube.ResourceKey]string{}
	for i, target := range rec.Target {
		l := rec.Live[i]
		if target == nil || l == nil || tracking.Owns(appID, l) {
			continue
		}
		key := kube.GetResourceKey(l)
		owner := tracking.AppOf(l)
		switch owner {
		case "":
			foreign[key] = fmt.Sprintf("%s %s already exists in the cluster and was not created by this application; SDI does not adopt, change or prune it", l.GetKind(), describeKey(key))
		default:
			foreign[key] = fmt.Sprintf("%s %s belongs to another application; SDI does not adopt, change or prune it", l.GetKind(), describeKey(key))
		}
	}
	diffs, err := diff.DiffArray(rec.Target, rec.Live, s.diffOptions(h.Cache())...)
	if err != nil {
		return nil, fmt.Errorf("the desired state could not be compared with the cluster: %s", oneLine(err.Error()))
	}
	c := &Comparison{Desired: d, Reconciliation: rec, Diffs: diffs.Diffs, Foreign: foreign,
		waves: map[kube.ResourceKey]int32{}, wavesByName: map[kube.ResourceKey]int32{}}
	for _, obj := range d.Objects {
		key := kube.GetResourceKey(obj)
		c.waves[key] = wave(obj)
		c.wavesByName[kube.ResourceKey{Group: key.Group, Kind: key.Kind, Name: key.Name}] = wave(obj)
	}
	c.Status = summarize(rec, diffs.Diffs, foreign)
	c.Status.Revision = d.Revision
	c.Status.DesiredDigest = d.Digest
	c.Status.Conditions = append(append([]string{}, d.Warnings...), sortedValues(foreign)...)
	c.Status.ReconciledAt = time.Now().UTC()
	return c, nil
}

// summarize computes every resource's sync and health status and the
// application's (CONTRACT.md §24.4): synced when every non-hook resource is;
// health the worst of the resources'.
func summarize(rec enginesync.ReconciliationResult, diffs []diff.DiffResult, foreign map[kube.ResourceKey]string) store.Status {
	st := store.Status{SyncStatus: SyncSynced, HealthStatus: HealthHealthy}
	worst := health.HealthStatusHealthy
	for i := range rec.Target {
		target, live := rec.Target[i], rec.Live[i]
		obj := live
		if obj == nil {
			obj = target
		}
		if obj == nil {
			continue
		}
		if target != nil && hookutil.Skip(target) {
			continue
		}
		gvk := obj.GroupVersionKind()
		r := store.ResourceStatus{
			Group: gvk.Group, Version: gvk.Version, Kind: gvk.Kind,
			Namespace: obj.GetNamespace(), Name: obj.GetName(),
		}
		isHook := live != nil && target == nil && (hookutil.IsHook(live) || ignore.Ignore(live))
		modified := i < len(diffs) && diffs[i].Modified
		r.Hook = isHook
		r.SyncStatus = resourceSyncStatus(target, live, modified, foreign[kube.GetResourceKey(obj)] != "", isHook)
		if target != nil {
			r.SyncWave = wave(target)
		} else if isHook {
			r.SyncWave = wave(live)
			for _, h := range rec.Hooks {
				if h.GetName() == live.GetName() && h.GetKind() == live.GetKind() && h.GetNamespace() == live.GetNamespace() {
					r.SyncWave = wave(h)
					break
				}
			}
		}
		code, message := resourceHealth(live)
		r.HealthStatus, r.HealthMessage = healthName(code), message
		if msg := foreign[kube.GetResourceKey(obj)]; msg != "" {
			r.HealthMessage = msg
		}
		st.Resources = append(st.Resources, r)
		if isHook {
			continue
		}
		st.ResourceCount++
		if r.SyncStatus != SyncSynced {
			st.OutOfSyncCount++
			st.SyncStatus = SyncOutOfSync
		}
		if code != "" && health.IsWorse(worst, code) {
			worst = code
			st.HealthMessage = ""
			if message != "" {
				st.HealthMessage = fmt.Sprintf("%s %s: %s", r.Kind, r.Name, message)
			}
		}
	}
	st.HealthStatus = healthName(worst)
	sortResources(st.Resources)
	return st
}

// resourceSyncStatus is one resource's sync status.
func resourceSyncStatus(target, live *unstructured.Unstructured, modified, foreign, isHook bool) string {
	switch {
	case isHook:
		// Hooks run during syncs and are not part of the steady state.
		return SyncSynced
	case target != nil && live == nil:
		return SyncMissing
	case target == nil && live != nil:
		return SyncExtraneous
	case foreign || modified:
		return SyncOutOfSync
	}
	return SyncSynced
}

// resourceHealth is the engine's health of a live resource; "" for kinds
// without a health check (a ConfigMap), Missing for no live object.
func resourceHealth(live *unstructured.Unstructured) (health.HealthStatusCode, string) {
	if live == nil {
		return health.HealthStatusMissing, ""
	}
	hs, err := health.GetResourceHealth(live, nil)
	if err != nil {
		return health.HealthStatusUnknown, oneLine(err.Error())
	}
	if hs == nil {
		return "", ""
	}
	return hs.Status, hs.Message
}

// healthName is the engine's health code in the contract's vocabulary.
func healthName(code health.HealthStatusCode) string {
	switch code {
	case health.HealthStatusHealthy:
		return HealthHealthy
	case health.HealthStatusProgressing:
		return HealthProgressing
	case health.HealthStatusDegraded:
		return HealthDegraded
	case health.HealthStatusSuspended:
		return HealthSuspended
	case health.HealthStatusMissing:
		return HealthMissing
	case health.HealthStatusUnknown:
		return HealthUnknown
	}
	return ""
}

func sortResources(rs []store.ResourceStatus) {
	sort.SliceStable(rs, func(i, j int) bool {
		a, b := rs[i], rs[j]
		if a.Group != b.Group {
			return a.Group < b.Group
		}
		if a.Kind != b.Kind {
			return a.Kind < b.Kind
		}
		if a.Namespace != b.Namespace {
			return a.Namespace < b.Namespace
		}
		return a.Name < b.Name
	})
}

func sortedValues(m map[kube.ResourceKey]string) []string {
	out := make([]string, 0, len(m))
	for _, v := range m {
		out = append(out, v)
	}
	sort.Strings(out)
	return out
}

func oneLine(msg string) string {
	if i := strings.IndexByte(msg, '\n'); i >= 0 {
		msg = msg[:i]
	}
	if len(msg) > 400 {
		msg = msg[:400] + "..."
	}
	return msg
}

// Refresh renders and compares an application now and records what it
// found. A comparison that cannot finish keeps the last known statuses and
// records why in the conditions; the returned application says either.
func (s *Service) Refresh(ctx context.Context, app store.Application, hard bool) (store.Application, error) {
	cmp, conditions := s.CompareNow(ctx, app, hard, nil)
	if cmp == nil {
		if err := s.Store.SaveConditions(ctx, app.WorkspaceID, app.ID, conditions); err != nil {
			return app, err
		}
		app.Conditions = conditions
		return app, nil
	}
	if err := s.saveStatus(ctx, app, cmp.Status); err != nil {
		return app, err
	}
	return s.Store.GetApplication(ctx, app.WorkspaceID, app.ID)
}

// CompareNow acquires the cluster, renders and compares. On failure it
// returns nil and the conditions to record. When keep is set the handle is
// passed back to the caller through it (who must release it).
func (s *Service) CompareNow(ctx context.Context, app store.Application, hard bool, keep **clusters.Handle) (*Comparison, []string) {
	cluster, err := s.Store.GetCluster(ctx, app.WorkspaceID, app.ClusterID)
	if err != nil {
		return nil, []string{"The application's cluster could not be read."}
	}
	h, err := s.Clusters.Acquire(ctx, cluster)
	if err != nil {
		return nil, []string{sentence(err.Error())}
	}
	release := true
	defer func() {
		if release {
			h.Release()
		}
	}()
	d, err := s.Desired(ctx, app, app.Source, "", h, hard)
	if err != nil {
		return nil, []string{sentence(err.Error())}
	}
	cmp, err := s.compare(app, h, d)
	if err != nil {
		return nil, []string{sentence(err.Error())}
	}
	if keep != nil {
		*keep = h
		release = false
	}
	return cmp, nil
}

// saveStatus records a comparison and announces a change of sync or health
// status.
func (s *Service) saveStatus(ctx context.Context, app store.Application, st store.Status) error {
	if err := s.Store.SaveStatus(ctx, app.WorkspaceID, app.ID, st); err != nil {
		return err
	}
	if st.SyncStatus != app.SyncStatus || st.HealthStatus != app.HealthStatus || st.Revision != app.Revision {
		s.Events.Application(app.WorkspaceID, app.ID, app.ProjectID, "updated", st.SyncStatus, map[string]string{
			"sync_status":   st.SyncStatus,
			"health_status": st.HealthStatus,
			"revision":      st.Revision,
			"name":          app.Name,
		})
	}
	return nil
}

// sentence makes an error message read as a sentence.
func sentence(msg string) string {
	msg = strings.TrimSpace(msg)
	if msg == "" {
		return "The comparison failed."
	}
	msg = strings.ToUpper(msg[:1]) + msg[1:]
	if !strings.HasSuffix(msg, ".") {
		msg += "."
	}
	return msg
}

package gitops

import (
	"fmt"
	"strings"
	"testing"
	"time"

	"github.com/google/uuid"
	"k8s.io/apimachinery/pkg/apis/meta/v1/unstructured"
	"k8s.io/apimachinery/pkg/runtime/schema"

	"github.com/argoproj/argo-cd/gitops-engine/pkg/diff"
	enginesync "github.com/argoproj/argo-cd/gitops-engine/pkg/sync"
	"github.com/argoproj/argo-cd/gitops-engine/pkg/utils/kube"

	"github.com/f8fvwgzc/software-delivery-intelligence/services/gitops/internal/store"
	"github.com/f8fvwgzc/software-delivery-intelligence/services/gitops/internal/tracking"
)

func object(apiVersion, kind, ns, name string) *unstructured.Unstructured {
	u := &unstructured.Unstructured{Object: map[string]any{}}
	u.SetAPIVersion(apiVersion)
	u.SetKind(kind)
	u.SetNamespace(ns)
	u.SetName(name)
	return u
}

func withStatus(u *unstructured.Unstructured, status map[string]any) *unstructured.Unstructured {
	u.Object["status"] = status
	return u
}

func readyDeployment(ns, name string) *unstructured.Unstructured {
	d := object("apps/v1", "Deployment", ns, name)
	d.Object["spec"] = map[string]any{"replicas": int64(1)}
	d.SetGeneration(1)
	return withStatus(d, map[string]any{
		"observedGeneration": int64(1), "replicas": int64(1), "updatedReplicas": int64(1),
		"availableReplicas": int64(1), "readyReplicas": int64(1),
	})
}

func degradedDeployment(ns, name string) *unstructured.Unstructured {
	d := object("apps/v1", "Deployment", ns, name)
	d.Object["spec"] = map[string]any{"replicas": int64(1), "progressDeadlineSeconds": int64(1)}
	d.SetGeneration(1)
	return withStatus(d, map[string]any{
		"observedGeneration": int64(1), "replicas": int64(1),
		"conditions": []any{map[string]any{"type": "Progressing", "status": "False", "reason": "ProgressDeadlineExceeded", "message": "too slow"}},
	})
}

func hookJob(ns, name string) *unstructured.Unstructured {
	j := object("batch/v1", "Job", ns, name)
	j.SetAnnotations(map[string]string{"argocd.argoproj.io/hook": "PreSync"})
	return withStatus(j, map[string]any{"succeeded": int64(1), "conditions": []any{map[string]any{"type": "Complete", "status": "True"}}})
}

func find(st store.Status, kind, name string) store.ResourceStatus {
	for _, r := range st.Resources {
		if r.Kind == kind && r.Name == name {
			return r
		}
	}
	return store.ResourceStatus{}
}

func TestStatusAggregation(t *testing.T) {
	cm := object("v1", "ConfigMap", "shop", "settings")
	missing := object("v1", "ConfigMap", "shop", "new")
	drifted := object("v1", "ConfigMap", "shop", "drifted")
	gone := object("v1", "ConfigMap", "shop", "gone")
	foreign := object("v1", "ConfigMap", "shop", "theirs")
	rec := enginesync.ReconciliationResult{
		Target: []*unstructured.Unstructured{readyDeployment("shop", "web"), cm, missing, drifted, nil, foreign, nil},
		Live:   []*unstructured.Unstructured{readyDeployment("shop", "web"), cm, nil, drifted, gone, foreign, hookJob("shop", "migrate")},
	}
	diffs := []diff.DiffResult{{}, {}, {Modified: true}, {Modified: true}, {Modified: true}, {}, {}}
	foreignKeys := map[kube.ResourceKey]string{kube.GetResourceKey(foreign): "not ours"}
	st := summarize(rec, diffs, foreignKeys)

	cases := map[string]string{
		"Deployment/web": SyncSynced, "ConfigMap/settings": SyncSynced, "ConfigMap/new": SyncMissing,
		"ConfigMap/drifted": SyncOutOfSync, "ConfigMap/gone": SyncExtraneous, "ConfigMap/theirs": SyncOutOfSync,
		"Job/migrate": SyncSynced,
	}
	for key, want := range cases {
		kind, name, _ := strings.Cut(key, "/")
		if got := find(st, kind, name).SyncStatus; got != want {
			t.Errorf("%s: sync %q, want %q", key, got, want)
		}
	}
	if !find(st, "Job", "migrate").Hook {
		t.Error("a live hook is marked as one")
	}
	if find(st, "ConfigMap", "theirs").HealthMessage != "not ours" {
		t.Error("a foreign resource says why it is left alone")
	}
	if st.SyncStatus != SyncOutOfSync || st.ResourceCount != 6 || st.OutOfSyncCount != 4 {
		t.Errorf("app: %s, %d resources, %d out of sync", st.SyncStatus, st.ResourceCount, st.OutOfSyncCount)
	}
	if got := find(st, "Deployment", "web").HealthStatus; got != HealthHealthy {
		t.Errorf("deployment health %q", got)
	}
	if got := find(st, "ConfigMap", "settings").HealthStatus; got != "" {
		t.Errorf("a kind without a health check has none: %q", got)
	}
	// The worst resource decides: a missing one.
	if st.HealthStatus != HealthMissing {
		t.Errorf("app health %q, want missing", st.HealthStatus)
	}
}

func TestEverythingSyncedAndHealthyAndTheWorstHealthWins(t *testing.T) {
	cm := object("v1", "ConfigMap", "shop", "settings")
	rec := enginesync.ReconciliationResult{
		Target: []*unstructured.Unstructured{readyDeployment("shop", "web"), cm},
		Live:   []*unstructured.Unstructured{readyDeployment("shop", "web"), cm},
	}
	st := summarize(rec, []diff.DiffResult{{}, {}}, nil)
	if st.SyncStatus != SyncSynced || st.HealthStatus != HealthHealthy || st.OutOfSyncCount != 0 {
		t.Fatalf("got %s/%s/%d", st.SyncStatus, st.HealthStatus, st.OutOfSyncCount)
	}
	// A degraded deployment beats a healthy one, and hooks do not count.
	rec = enginesync.ReconciliationResult{
		Target: []*unstructured.Unstructured{readyDeployment("shop", "a"), degradedDeployment("shop", "b"), nil},
		Live:   []*unstructured.Unstructured{readyDeployment("shop", "a"), degradedDeployment("shop", "b"), hookJob("shop", "h")},
	}
	st = summarize(rec, []diff.DiffResult{{}, {}, {}}, nil)
	if st.HealthStatus != HealthDegraded || !strings.Contains(st.HealthMessage, "Deployment b") {
		t.Fatalf("health %s (%s)", st.HealthStatus, st.HealthMessage)
	}
	if st.SyncStatus != SyncSynced {
		t.Fatalf("hooks are not part of the steady state: %s", st.SyncStatus)
	}
	// No resources at all is healthy and synced.
	st = summarize(enginesync.ReconciliationResult{}, nil, nil)
	if st.SyncStatus != SyncSynced || st.HealthStatus != HealthHealthy {
		t.Fatalf("empty: %s/%s", st.SyncStatus, st.HealthStatus)
	}
}

type scopes map[string]bool

func (s scopes) IsNamespaced(gk schema.GroupKind) (bool, error) {
	v, ok := s[gk.String()]
	if !ok {
		return false, fmt.Errorf("unknown")
	}
	return v, nil
}

func TestNormalizePutsResourcesInPlaceTracksThemAndKeepsTheLastDuplicate(t *testing.T) {
	svc := &Service{}
	app := store.Application{ID: uuid.Must(uuid.NewV7()), Namespace: "shop"}
	first := object("v1", "ConfigMap", "", "settings")
	first.Object["data"] = map[string]any{"v": "1"}
	second := object("v1", "ConfigMap", "", "settings")
	second.Object["data"] = map[string]any{"v": "2"}
	ns := object("v1", "Namespace", "", "extra")
	pinned := object("v1", "ConfigMap", "other", "pinned")
	custom := object("example.com/v1", "Widget", "", "w")
	objs, warnings := svc.normalize(app, []*unstructured.Unstructured{first, second, ns, pinned, custom},
		scopes{"ConfigMap": true, "Namespace": false})
	if len(objs) != 4 || len(warnings) != 1 || !strings.Contains(warnings[0], "settings is defined more than once") {
		t.Fatalf("objects %d, warnings %v", len(objs), warnings)
	}
	byName := map[string]*unstructured.Unstructured{}
	for _, o := range objs {
		byName[o.GetName()] = o
		if !tracking.Owns(app.ID.String(), o) {
			t.Errorf("%s carries no tracking id of its application", o.GetName())
		}
	}
	if v, _, _ := unstructured.NestedString(byName["settings"].Object, "data", "v"); v != "2" {
		t.Errorf("the last duplicate wins: %s", v)
	}
	if byName["settings"].GetNamespace() != "shop" || byName["extra"].GetNamespace() != "" ||
		byName["pinned"].GetNamespace() != "other" || byName["w"].GetNamespace() != "shop" {
		t.Errorf("namespaces: %s %s %s %s", byName["settings"].GetNamespace(), byName["extra"].GetNamespace(),
			byName["pinned"].GetNamespace(), byName["w"].GetNamespace())
	}
	if digest(objs) != digest([]*unstructured.Unstructured{objs[3], objs[2], objs[1], objs[0]}) {
		t.Error("the digest does not depend on order")
	}
}

func TestBackoffDoublesFromTenSecondsUpToFiveMinutes(t *testing.T) {
	want := []time.Duration{10 * time.Second, 20 * time.Second, 40 * time.Second, 80 * time.Second, 160 * time.Second, 5 * time.Minute, 5 * time.Minute}
	for i, w := range want {
		if got := Backoff(i + 1); got != w {
			t.Errorf("Backoff(%d) = %s, want %s", i+1, got, w)
		}
	}
}

func outOfSync(digest string, statuses ...string) store.Status {
	st := store.Status{SyncStatus: SyncOutOfSync, DesiredDigest: digest, Revision: "abcdef0123"}
	for i, s := range statuses {
		st.Resources = append(st.Resources, store.ResourceStatus{Kind: "ConfigMap", Name: fmt.Sprint(i), SyncStatus: s})
	}
	return st
}

func TestAutomationDecisions(t *testing.T) {
	now := time.Now()
	later := now.Add(time.Minute)
	auto := store.SyncPolicy{Automated: true}
	cases := []struct {
		name     string
		in       AutoInput
		sync     bool
		selfHeal bool
		cond     bool
	}{
		{"off", AutoInput{Policy: store.SyncPolicy{}, Status: outOfSync("d2", SyncOutOfSync), SyncedDigest: "d1"}, false, false, false},
		{"synced", AutoInput{Policy: auto, Status: store.Status{SyncStatus: SyncSynced}}, false, false, false},
		{"unknown", AutoInput{Policy: auto, Status: store.Status{SyncStatus: SyncUnknown}}, false, false, false},
		{"operation running", AutoInput{Policy: auto, Status: outOfSync("d2", SyncOutOfSync), SyncedDigest: "d1", OperationRunning: true}, false, false, false},
		{"new revision", AutoInput{Policy: auto, Status: outOfSync("d2", SyncOutOfSync), SyncedDigest: "d1"}, true, false, false},
		{"never synced", AutoInput{Policy: auto, Status: outOfSync("d1", SyncMissing)}, true, false, false},
		{"drift without self-heal", AutoInput{Policy: auto, Status: outOfSync("d1", SyncOutOfSync), SyncedDigest: "d1"}, false, false, false},
		{"drift with self-heal", AutoInput{Policy: store.SyncPolicy{Automated: true, SelfHeal: true}, Status: outOfSync("d1", SyncOutOfSync, SyncSynced), SyncedDigest: "d1"}, true, true, false},
		{"only prune, prune off", AutoInput{Policy: auto, Status: outOfSync("d2", SyncSynced, SyncExtraneous), SyncedDigest: "d1"}, false, false, false},
		{"only prune, prune on", AutoInput{Policy: store.SyncPolicy{Automated: true, Prune: true}, Status: outOfSync("d2", SyncSynced, SyncExtraneous), SyncedDigest: "d1"}, true, false, false},
		{"would wipe everything", AutoInput{Policy: store.SyncPolicy{Automated: true, Prune: true}, Status: outOfSync("d2", SyncExtraneous, SyncExtraneous), SyncedDigest: "d1"}, false, false, true},
		{"retry not yet due", AutoInput{Policy: auto, Status: outOfSync("d2", SyncOutOfSync), SyncedDigest: "d1", AutoDigest: "d2", AutoAttempts: 1, AutoNextAt: &later, Now: now}, false, false, false},
		{"retry due", AutoInput{Policy: auto, Status: outOfSync("d2", SyncOutOfSync), SyncedDigest: "d1", AutoDigest: "d2", AutoAttempts: 3, AutoNextAt: &now, Now: later}, true, false, false},
		{"retries exhausted", AutoInput{Policy: auto, Status: outOfSync("d2", SyncOutOfSync), SyncedDigest: "d1", AutoDigest: "d2", AutoAttempts: 4, AutoNextAt: &now, Now: later}, false, false, true},
		{"retry limit from the policy", AutoInput{Policy: store.SyncPolicy{Automated: true, RetryLimit: 1}, Status: outOfSync("d2", SyncOutOfSync), SyncedDigest: "d1", AutoDigest: "d2", AutoAttempts: 2, Now: later}, false, false, true},
		{"a new revision resets the retries", AutoInput{Policy: auto, Status: outOfSync("d3", SyncOutOfSync), SyncedDigest: "d1", AutoDigest: "d2", AutoAttempts: 9, Now: later}, true, false, false},
	}
	for _, c := range cases {
		t.Run(c.name, func(t *testing.T) {
			if c.in.Now.IsZero() {
				c.in.Now = now
			}
			d := Decide(c.in)
			if d.Sync != c.sync || d.SelfHeal != c.selfHeal || (d.Condition != "") != c.cond {
				t.Fatalf("got %+v", d)
			}
		})
	}
}

func TestAfterAttemptCountsFailuresPerDesiredState(t *testing.T) {
	now := time.Now()
	in := AutoInput{Status: store.Status{DesiredDigest: "d2"}, AutoDigest: "d2", AutoAttempts: 2, Now: now}
	digest, attempts, next := AfterAttempt(in, false)
	if digest != "d2" || attempts != 3 || next == nil || !next.Equal(now.Add(40*time.Second)) {
		t.Fatalf("failure: %s %d %v", digest, attempts, next)
	}
	in.AutoDigest = "d1"
	if _, attempts, next = AfterAttempt(in, false); attempts != 1 || !next.Equal(now.Add(10*time.Second)) {
		t.Fatalf("a new state starts counting again: %d %v", attempts, next)
	}
	if digest, attempts, next = AfterAttempt(in, true); digest != "" || attempts != 0 || next != nil {
		t.Fatalf("success resets: %s %d %v", digest, attempts, next)
	}
}

func TestOverridesMergeByKey(t *testing.T) {
	src := store.Source{KustomizeImages: []string{"web=registry/web:1", "worker:2"}, HelmParameters: map[string]string{"a": "1"}}
	out, err := MergeOverrides(src, []string{"web=registry/web:3", "cache:7"}, map[string]string{"image.tag": "3", "a": "2"})
	if err != nil {
		t.Fatal(err)
	}
	if strings.Join(out.KustomizeImages, ",") != "web=registry/web:3,worker:2,cache:7" {
		t.Errorf("images %v", out.KustomizeImages)
	}
	if out.HelmParameters["a"] != "2" || out.HelmParameters["image.tag"] != "3" {
		t.Errorf("params %v", out.HelmParameters)
	}
	if src.KustomizeImages[0] != "web=registry/web:1" || src.HelmParameters["a"] != "1" {
		t.Error("the input source was modified")
	}
	if _, err := MergeOverrides(src, []string{"nonsense"}, nil); err == nil {
		t.Error("an invalid override was merged")
	}
}

func TestTheManifestCacheIsBoundedAndExpires(t *testing.T) {
	c := NewManifestCache(time.Hour, 2, 100)
	objs := []*unstructured.Unstructured{object("v1", "ConfigMap", "a", "b")}
	c.Put("a", "directory", objs, 10)
	c.Put("b", "directory", objs, 10)
	c.Put("c", "directory", objs, 10)
	if _, _, ok := c.Get("a"); ok || c.Len() != 2 {
		t.Fatalf("entries bound: len %d", c.Len())
	}
	got, kind, ok := c.Get("c")
	if !ok || kind != "directory" || len(got) != 1 {
		t.Fatal("a cached render comes back")
	}
	got[0].SetName("changed")
	again, _, _ := c.Get("c")
	if again[0].GetName() != "b" {
		t.Fatal("callers get copies")
	}
	c.Put("big", "directory", objs, 90)
	if c.Bytes() > 100 {
		t.Fatalf("byte budget exceeded: %d", c.Bytes())
	}
	c.Put("huge", "directory", objs, 101)
	if _, _, ok := c.Get("huge"); ok {
		t.Fatal("an entry larger than the budget is not kept")
	}
	c.Delete("big")
	if _, _, ok := c.Get("big"); ok {
		t.Fatal("Delete drops (hard refresh)")
	}
	c.Put("ws1|x", "directory", objs, 1)
	c.DeletePrefix("ws1|")
	if _, _, ok := c.Get("ws1|x"); ok {
		t.Fatal("DeletePrefix drops a workspace")
	}
	short := NewManifestCache(time.Millisecond, 10, 100)
	short.Put("k", "directory", objs, 1)
	time.Sleep(5 * time.Millisecond)
	if _, _, ok := short.Get("k"); ok {
		t.Fatal("entries expire")
	}
}

func TestManifestsSortByWaveThenKindOrder(t *testing.T) {
	ns := object("v1", "Namespace", "", "shop")
	dep := object("apps/v1", "Deployment", "shop", "web")
	cm := object("v1", "ConfigMap", "shop", "settings")
	late := object("v1", "ConfigMap", "shop", "late")
	late.SetAnnotations(map[string]string{"argocd.argoproj.io/sync-wave": "5"})
	early := object("apps/v1", "Deployment", "shop", "early")
	early.SetAnnotations(map[string]string{"argocd.argoproj.io/sync-wave": "-1"})
	custom := object("example.com/v1", "Widget", "shop", "w")
	objs := []*unstructured.Unstructured{late, custom, dep, cm, early, ns}
	SortManifests(objs)
	var names []string
	for _, o := range objs {
		names = append(names, o.GetName())
	}
	if got := strings.Join(names, ","); got != "early,shop,settings,web,w,late" {
		t.Fatalf("order %s", got)
	}
}

func TestSelectedMatchesPartialSyncs(t *testing.T) {
	key := kube.NewResourceKey("apps", "Deployment", "shop", "web")
	if !selected(nil, key) {
		t.Fatal("no refs selects everything")
	}
	if !selected([]ResourceRef{{Group: "apps", Kind: "Deployment", Name: "web"}}, key) {
		t.Fatal("a ref without a namespace matches any")
	}
	if selected([]ResourceRef{{Group: "apps", Kind: "Deployment", Namespace: "other", Name: "web"}}, key) {
		t.Fatal("the namespace must match when given")
	}
	if selected([]ResourceRef{{Kind: "Deployment", Name: "web"}}, key) {
		t.Fatal("the group must match")
	}
}

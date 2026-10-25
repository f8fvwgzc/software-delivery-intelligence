package integration

import (
	"context"
	"strings"
	"testing"
	"time"

	metav1 "k8s.io/apimachinery/pkg/apis/meta/v1"

	gitopsv1 "github.com/f8fvwgzc/software-delivery-intelligence/services/gitops/gen/sdi/gitops/v1"
	"github.com/f8fvwgzc/software-delivery-intelligence/services/gitops/internal/gitops"
	"github.com/f8fvwgzc/software-delivery-intelligence/services/gitops/internal/principal"
)

// TestKustomizeWithAnImageOverride: the overlay's Deployment only runs once
// the override replaces its placeholder image; the override is stored into
// the source by the sync, so the desired state stays declared.
func TestKustomizeWithAnImageOverride(t *testing.T) {
	e := newEnv(t)
	ns := e.namespace()
	c := e.cluster()
	app := e.app(c, ns, &gitopsv1.ApplicationSource{Path: "kustomize/overlay"}, &gitopsv1.SyncPolicy{CreateNamespace: true})
	a := e.refresh(app.GetId())
	if a.GetSyncStatus() != "out_of_sync" || resourceOf(a, "Deployment", "test-api") == nil {
		t.Fatalf("kustomize comparison: %v %v", a.GetResources(), a.GetConditions())
	}
	res := e.sync(&gitopsv1.SyncApplicationRequest{Id: app.GetId(), KustomizeImages: []string{"placeholder=registry.k8s.io/pause:3.10"}})
	if res.GetPhase() != "succeeded" {
		t.Fatalf("sync: %s", describeResults(res))
	}
	got, err := e.client.GetApplication(e.as(principal.Viewer), &gitopsv1.GetApplicationRequest{Id: app.GetId()})
	if err != nil || len(got.GetSource().GetKustomizeImages()) != 1 || got.GetSource().GetKustomizeImages()[0] != "placeholder=registry.k8s.io/pause:3.10" {
		t.Fatalf("the override is stored into the source: %v %v", got.GetSource(), err)
	}
	d, err := e.kube.AppsV1().Deployments(ns).Get(context.Background(), "test-api", metav1.GetOptions{})
	if err != nil {
		t.Fatal(err)
	}
	if img := d.Spec.Template.Spec.Containers[0].Image; img != "registry.k8s.io/pause:3.10" || d.Labels["variant"] != "overlay" {
		t.Fatalf("live deployment: %s %v", img, d.Labels)
	}
	e.waitHealthy(app.GetId())
	// A later sync without overrides keeps the stored one.
	res = e.sync(&gitopsv1.SyncApplicationRequest{Id: app.GetId()})
	if res.GetPhase() != "succeeded" || resultOf(res, "Deployment", "test-api").GetStatus() != "unchanged" {
		t.Fatalf("second sync: %s", describeResults(res))
	}
}

// TestHelmChartWithValuesAndParameters: values files in order, parameters
// on top, the release name and namespace, and a sync that sets a parameter.
func TestHelmChartWithValuesAndParameters(t *testing.T) {
	e := newEnv(t)
	ns := e.namespace()
	c := e.cluster()
	app := e.app(c, ns, &gitopsv1.ApplicationSource{
		Path: "helm/chart", HelmValueFiles: []string{"values-test.yaml"},
		HelmParameters: map[string]string{"replicaCount": "2"}, HelmReleaseName: "shop",
	}, &gitopsv1.SyncPolicy{CreateNamespace: true})
	res := e.sync(&gitopsv1.SyncApplicationRequest{Id: app.GetId(), HelmParameters: map[string]string{"message": "from-sync"}})
	if res.GetPhase() != "succeeded" {
		t.Fatalf("sync: %s", describeResults(res))
	}
	d, err := e.kube.AppsV1().Deployments(ns).Get(context.Background(), "shop", metav1.GetOptions{})
	if err != nil {
		t.Fatal(err)
	}
	if *d.Spec.Replicas != 2 || d.Labels["tier"] != "test" {
		t.Fatalf("parameters and values files: replicas %d, labels %v", *d.Spec.Replicas, d.Labels)
	}
	cm := e.configMap(ns, "shop-config")
	if cm.Data["message"] != "from-sync" || cm.Data["namespace"] != ns {
		t.Fatalf("configmap %v", cm.Data)
	}
	got, _ := e.client.GetApplication(e.as(principal.Viewer), &gitopsv1.GetApplicationRequest{Id: app.GetId()})
	params := got.GetSource().GetHelmParameters()
	if params["replicaCount"] != "2" || params["message"] != "from-sync" {
		t.Fatalf("parameters merged into the source: %v", params)
	}
	e.waitHealthy(app.GetId())
}

// TestSyncWavesAndAPreSyncHook: the PreSync Job completes before wave -1,
// and waves run in order.
func TestSyncWavesAndAPreSyncHook(t *testing.T) {
	e := newEnv(t)
	ns := e.namespace()
	c := e.cluster()
	app := e.app(c, ns, &gitopsv1.ApplicationSource{Path: "waves"}, &gitopsv1.SyncPolicy{CreateNamespace: true})
	res := e.sync(&gitopsv1.SyncApplicationRequest{Id: app.GetId()})
	if res.GetPhase() != "succeeded" {
		t.Fatalf("sync: %s", describeResults(res))
	}
	hook := resultOf(res, "Job", "migrate")
	if hook == nil || hook.GetHookPhase() != "pre_sync" || hook.GetStatus() != "synced" {
		t.Fatalf("the PreSync hook ran: %s", describeResults(res))
	}
	if w := resultOf(res, "ConfigMap", "first").GetSyncWave(); w != -1 {
		t.Fatalf("wave of first: %d", w)
	}
	if w := resultOf(res, "ConfigMap", "last").GetSyncWave(); w != 1 {
		t.Fatalf("wave of last: %d", w)
	}
	got := strings.Join(e.waves.of(app.GetId()), " ")
	if got != "PreSync/0 Sync/-1 Sync/0 Sync/1" {
		t.Fatalf("waves ran as %q", got)
	}
	job, err := e.kube.BatchV1().Jobs(ns).Get(context.Background(), "migrate", metav1.GetOptions{})
	if err != nil {
		t.Fatal(err)
	}
	first := e.configMap(ns, "first")
	last := e.configMap(ns, "last")
	if job.Status.CompletionTime == nil || job.Status.CompletionTime.After(first.CreationTimestamp.Time) {
		t.Fatalf("the hook completed (%v) before wave -1 was applied (%v)", job.Status.CompletionTime, first.CreationTimestamp)
	}
	d, err := e.kube.AppsV1().Deployments(ns).Get(context.Background(), "middle", metav1.GetOptions{})
	if err != nil {
		t.Fatal(err)
	}
	if d.CreationTimestamp.Before(&first.CreationTimestamp) || last.CreationTimestamp.Before(&d.CreationTimestamp) {
		t.Fatalf("creation order: first %v, middle %v, last %v", first.CreationTimestamp, d.CreationTimestamp, last.CreationTimestamp)
	}
	a := e.waitHealthy(app.GetId())
	if r := resourceOf(a, "Job", "migrate"); r == nil || !r.GetHook() || r.GetSyncStatus() != "synced" {
		t.Fatalf("the live hook is listed as a hook, not part of the steady state: %v", r)
	}
	// A partial sync skips hooks and touches only what it names.
	res = e.sync(&gitopsv1.SyncApplicationRequest{Id: app.GetId(), Resources: []*gitopsv1.ResourceRef{{Kind: "ConfigMap", Name: "last"}}})
	if res.GetPhase() != "succeeded" || len(res.GetResources()) != 1 || resultOf(res, "ConfigMap", "last") == nil {
		t.Fatalf("partial sync: %s", describeResults(res))
	}
}

// TestAutomatedSyncRetriesWithBackoff: an automated sync that fails is not
// retried before its backoff, then is.
func TestAutomatedSyncRetriesWithBackoff(t *testing.T) {
	e := newEnv(t)
	ns := e.namespace()
	c := e.cluster()
	e.github.commit(func(files map[string][]byte) {
		// The API server refuses this name: every sync fails.
		files["directory/configmap.yaml"] = []byte("apiVersion: v1\nkind: ConfigMap\nmetadata:\n  name: Bad_Name\ndata:\n  a: b\n")
	})
	app := e.app(c, ns, &gitopsv1.ApplicationSource{Path: "directory"}, &gitopsv1.SyncPolicy{Automated: true, CreateNamespace: true, RetryLimit: 2})
	ref := gitops.Ref(e.workspace, mustUUID(t, app.GetId()))
	e.reconciler.ReconcileOne(context.Background(), ref)
	stored, err := e.store.GetApplication(context.Background(), e.workspace, ref.ID)
	if err != nil {
		t.Fatal(err)
	}
	if stored.AutoAttempts != 1 || stored.AutoNextAt == nil || time.Until(*stored.AutoNextAt) > 11*time.Second {
		t.Fatalf("after the first failure: attempts %d, next %v", stored.AutoAttempts, stored.AutoNextAt)
	}
	history := func() int {
		h, _ := e.client.ListHistory(e.as(principal.Viewer), &gitopsv1.ListHistoryRequest{Id: app.GetId()})
		return len(h.GetEntries())
	}
	if n := history(); n != 1 {
		t.Fatalf("one automated attempt recorded, got %d", n)
	}
	h, _ := e.client.ListHistory(e.as(principal.Viewer), &gitopsv1.ListHistoryRequest{Id: app.GetId()})
	if h.GetEntries()[0].GetPhase() == "succeeded" || !h.GetEntries()[0].GetAutomated() {
		t.Fatalf("the attempt failed and was automated: %v", h.GetEntries()[0])
	}
	// Before the backoff: nothing.
	e.reconciler.ReconcileOne(context.Background(), ref)
	if n := history(); n != 1 {
		t.Fatalf("retried before the backoff ended: %d attempts", n)
	}
	// After it: the retry.
	time.Sleep(time.Until(*stored.AutoNextAt) + 500*time.Millisecond)
	e.reconciler.ReconcileOne(context.Background(), ref)
	if n := history(); n != 2 {
		t.Fatalf("the retry after the backoff: %d attempts", n)
	}
	stored, _ = e.store.GetApplication(context.Background(), e.workspace, ref.ID)
	if stored.AutoAttempts != 2 || time.Until(*stored.AutoNextAt) < 15*time.Second {
		t.Fatalf("the second backoff doubles: attempts %d, next in %s", stored.AutoAttempts, time.Until(*stored.AutoNextAt))
	}
	// A fixed commit resets the count and syncs.
	e.github.commit(func(files map[string][]byte) {
		files["directory/configmap.yaml"] = []byte("apiVersion: v1\nkind: ConfigMap\nmetadata:\n  name: settings\ndata:\n  a: fixed\n")
	})
	e.reconciler.ReconcileOne(context.Background(), ref)
	stored, _ = e.store.GetApplication(context.Background(), e.workspace, ref.ID)
	if stored.AutoAttempts != 0 || stored.Operation == nil || stored.Operation.Phase != "succeeded" {
		t.Fatalf("a new revision syncs and resets: attempts %d, op %+v", stored.AutoAttempts, stored.Operation)
	}
	if e.configMap(ns, "settings").Data["a"] != "fixed" {
		t.Fatal("the fixed commit was not applied")
	}
}

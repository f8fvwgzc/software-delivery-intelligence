package integration

import (
	"context"
	"fmt"
	"io"
	"log/slog"
	"strings"
	"testing"
	"time"

	"github.com/google/uuid"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/status"
	corev1 "k8s.io/api/core/v1"
	apierrors "k8s.io/apimachinery/pkg/api/errors"
	metav1 "k8s.io/apimachinery/pkg/apis/meta/v1"

	gitopsv1 "github.com/f8fvwgzc/software-delivery-intelligence/services/gitops/gen/sdi/gitops/v1"
	"github.com/f8fvwgzc/software-delivery-intelligence/services/gitops/internal/clusters"
	"github.com/f8fvwgzc/software-delivery-intelligence/services/gitops/internal/gitops"
	"github.com/f8fvwgzc/software-delivery-intelligence/services/gitops/internal/logx"
	"github.com/f8fvwgzc/software-delivery-intelligence/services/gitops/internal/netguard"
	"github.com/f8fvwgzc/software-delivery-intelligence/services/gitops/internal/principal"
	"github.com/f8fvwgzc/software-delivery-intelligence/services/gitops/internal/tracking"
)

func (e *env) configMap(ns, name string) *corev1.ConfigMap {
	e.t.Helper()
	cm, err := e.kube.CoreV1().ConfigMaps(ns).Get(context.Background(), name, metav1.GetOptions{})
	if err != nil {
		e.t.Fatalf("ConfigMap %s/%s: %v", ns, name, err)
	}
	return cm
}

func (e *env) gone(ns, kind, name string, get func() error) {
	e.t.Helper()
	e.eventually(time.Minute, kind+" "+name+" deleted", func() (bool, string) {
		err := get()
		return apierrors.IsNotFound(err), fmt.Sprint(err)
	})
}

// TestClusterRegistration covers §24.2: a cluster is reached when it is
// saved, a bad one is saved anyway with the reason, and the address guard
// refuses the loopback API server unless private targets are allowed.
func TestClusterRegistration(t *testing.T) {
	e := newEnv(t)
	c := e.cluster()
	if !strings.HasPrefix(c.GetServerVersion(), "v1.") || !strings.HasPrefix(c.GetServer(), "https://127.0.0.1") || c.GetCheckedAt() == nil {
		t.Fatalf("cluster %+v", c)
	}
	e.identity.mu.Lock()
	resolves := append([]string(nil), e.identity.resolves...)
	e.identity.mu.Unlock()
	if len(resolves) == 0 || !strings.HasPrefix(resolves[0], principal.SystemUser+"/") {
		t.Fatalf("the kubeconfig is resolved as the workspace's system principal: %v", resolves)
	}
	bad, err := e.client.UpsertCluster(e.as(principal.Admin), &gitopsv1.UpsertClusterRequest{Cluster: &gitopsv1.Cluster{
		Name: "missing-" + randomHex(3), KubeconfigSecret: "NO_SUCH_SECRET",
	}})
	if err != nil {
		t.Fatalf("an unreachable cluster is still saved: %v", err)
	}
	if bad.GetStatus() != "unreachable" || !strings.Contains(bad.GetMessage(), "NO_SUCH_SECRET") {
		t.Fatalf("bad cluster: %s %q", bad.GetStatus(), bad.GetMessage())
	}
	list, err := e.client.ListClusters(e.as(principal.Viewer), &gitopsv1.ListClustersRequest{})
	if err != nil || len(list.GetClusters()) != 2 {
		t.Fatalf("list: %v %v", list, err)
	}
	for _, cl := range list.GetClusters() {
		if strings.Contains(cl.String(), "client-key-data") || strings.Contains(cl.String(), "BEGIN") {
			t.Fatal("a cluster answer carries kubeconfig material")
		}
	}
	// The guard: the same kubeconfig is refused without the flag.
	log := slog.New(slog.NewTextHandler(io.Discard, nil))
	strict := clusters.NewManager(fakeResolver{e}, netguard.New(netguard.Policy{}), 4, log, logx.Engine(log))
	cl, _ := e.store.GetCluster(context.Background(), e.workspace, mustUUID(t, c.GetId()))
	if _, _, err := strict.Check(context.Background(), cl); err == nil || !strings.Contains(err.Error(), "loopback") {
		t.Fatalf("a loopback API server must be refused by default: %v", err)
	}
	if _, err := e.client.DeleteCluster(e.as(principal.Admin), &gitopsv1.DeleteClusterRequest{Id: bad.GetId()}); err != nil {
		t.Fatalf("delete: %v", err)
	}
}

type fakeResolver struct{ e *env }

func (f fakeResolver) ResolveSecret(context.Context, uuid.UUID, string) (string, error) {
	return string(f.e.kubeconfig), nil
}

// TestDirectoryApplicationLifecycle walks a plain-manifest application
// through everything §24.3–24.6 promise against the cluster.
func TestDirectoryApplicationLifecycle(t *testing.T) {
	e := newEnv(t)
	ns := e.namespace()
	c := e.cluster()
	app := e.app(c, ns, &gitopsv1.ApplicationSource{Path: "directory"}, &gitopsv1.SyncPolicy{CreateNamespace: true})
	first := e.github.headSHA()

	// Compare: everything missing.
	a := e.refresh(app.GetId())
	if a.GetSyncStatus() != "out_of_sync" || a.GetRevision() != first || len(a.GetResources()) != 3 || a.GetHealthStatus() != "missing" {
		t.Fatalf("first comparison: %s/%s rev %s, %d resources, %v", a.GetSyncStatus(), a.GetHealthStatus(), a.GetRevision(), len(a.GetResources()), a.GetConditions())
	}
	for _, r := range a.GetResources() {
		if r.GetSyncStatus() != "missing" {
			t.Errorf("%s %s is %s, want missing", r.GetKind(), r.GetName(), r.GetSyncStatus())
		}
	}
	diff, err := e.client.DiffApplication(e.as(principal.Viewer), &gitopsv1.DiffApplicationRequest{Id: app.GetId()})
	if err != nil || len(diff.GetResources()) != 3 {
		t.Fatalf("diff: %v %v", diff, err)
	}
	for _, d := range diff.GetResources() {
		if d.GetLive() != "" || d.GetTarget() == "" || d.GetDiff() == "" {
			t.Errorf("a missing %s diffs from nothing: live %q", d.GetKind(), d.GetLive())
		}
	}
	manifests, err := e.client.GetManifests(e.as(principal.Viewer), &gitopsv1.GetManifestsRequest{Id: app.GetId()})
	if err != nil || len(manifests.GetManifests()) != 3 || manifests.GetRevision() != first {
		t.Fatalf("manifests: %v", err)
	}
	if !strings.Contains(manifests.GetManifests()[0], "kind: ConfigMap") || !strings.Contains(manifests.GetManifests()[2], "kind: Deployment") ||
		!strings.Contains(manifests.GetManifests()[0], tracking.Annotation+": "+app.GetId()+":/ConfigMap:"+ns+"/settings") {
		t.Fatalf("manifests are sorted by kind order and tracked:\n%s", strings.Join(manifests.GetManifests(), "---\n"))
	}

	// A dry run changes nothing and leaves no history.
	dry := e.sync(&gitopsv1.SyncApplicationRequest{Id: app.GetId(), DryRun: true})
	if dry.GetPhase() != "succeeded" || dry.GetHistoryId() != "" || len(dry.GetResources()) < 3 {
		t.Fatalf("dry run: %s", describeResults(dry))
	}
	if _, err := e.kube.CoreV1().Namespaces().Get(context.Background(), ns, metav1.GetOptions{}); !apierrors.IsNotFound(err) {
		t.Fatalf("a dry run created the namespace: %v", err)
	}
	if h, _ := e.client.ListHistory(e.as(principal.Viewer), &gitopsv1.ListHistoryRequest{Id: app.GetId()}); len(h.GetEntries()) != 0 {
		t.Fatalf("a dry run wrote history: %v", h.GetEntries())
	}

	// Sync: created, healthy, recorded.
	res := e.sync(&gitopsv1.SyncApplicationRequest{Id: app.GetId(), ExecutionId: "exec-1"})
	if res.GetPhase() != "succeeded" || res.GetRevision() != first || res.GetHistoryId() == "" {
		t.Fatalf("sync: %s", describeResults(res))
	}
	for _, kind := range []string{"ConfigMap", "Service", "Deployment"} {
		var r *gitopsv1.ResourceResult
		for _, rr := range res.GetResources() {
			if rr.GetKind() == kind {
				r = rr
			}
		}
		if r == nil || r.GetStatus() != "synced" {
			t.Errorf("%s: %v", kind, r)
		}
	}
	settings := e.configMap(ns, "settings")
	if settings.Data["greeting"] != "hello-v1" || settings.Annotations[tracking.Annotation] != app.GetId()+":/ConfigMap:"+ns+"/settings" {
		t.Fatalf("live ConfigMap: %v %v", settings.Data, settings.Annotations)
	}
	a = e.waitHealthy(app.GetId())
	if a.GetSyncedRevision() != first || a.GetOperation().GetPhase() != "succeeded" || a.GetOperation().GetExecutionId() != "exec-1" {
		t.Fatalf("after sync: %+v", a.GetOperation())
	}

	// A new commit: out of sync, then synced to it.
	second := e.github.commit(func(files map[string][]byte) {
		files["directory/configmap.yaml"] = []byte(strings.Replace(string(files["directory/configmap.yaml"]), "hello-v1", "hello-v2", 1))
	})
	a = e.refresh(app.GetId())
	if a.GetSyncStatus() != "out_of_sync" || a.GetRevision() != second || resourceOf(a, "ConfigMap", "settings").GetSyncStatus() != "out_of_sync" ||
		resourceOf(a, "Deployment", "web").GetSyncStatus() != "synced" {
		t.Fatalf("after a commit: %s, %v", a.GetSyncStatus(), a.GetResources())
	}
	diff, _ = e.client.DiffApplication(e.as(principal.Viewer), &gitopsv1.DiffApplicationRequest{Id: app.GetId()})
	for _, d := range diff.GetResources() {
		if d.GetKind() == "ConfigMap" && !strings.Contains(d.GetDiff(), "+  greeting: hello-v2") {
			t.Errorf("the diff shows the change:\n%s", d.GetDiff())
		}
		if d.GetKind() == "Deployment" && d.GetDiff() != "" {
			t.Errorf("an unchanged Deployment has no diff (status is normalised away):\n%s", d.GetDiff())
		}
	}
	res = e.sync(&gitopsv1.SyncApplicationRequest{Id: app.GetId()})
	if res.GetPhase() != "succeeded" || e.configMap(ns, "settings").Data["greeting"] != "hello-v2" {
		t.Fatalf("sync to the second commit: %s", describeResults(res))
	}

	// Rollback restores the first commit's state.
	history, err := e.client.ListHistory(e.as(principal.Viewer), &gitopsv1.ListHistoryRequest{Id: app.GetId()})
	if err != nil || len(history.GetEntries()) != 2 || history.GetEntries()[1].GetRevision() != first || history.GetEntries()[1].GetExecutionId() != "exec-1" {
		t.Fatalf("history: %v %v", history, err)
	}
	if history.GetEntries()[1].GetSource().GetRevision() != first {
		t.Fatalf("history pins the source to the synced commit: %v", history.GetEntries()[1].GetSource())
	}
	rb, err := e.client.RollbackApplication(e.as(principal.Approver), &gitopsv1.RollbackApplicationRequest{Id: app.GetId(), HistoryId: history.GetEntries()[1].GetId()})
	if err != nil || rb.GetPhase() != "succeeded" || rb.GetRevision() != first {
		t.Fatalf("rollback: %v %v", rb, err)
	}
	if got := e.configMap(ns, "settings").Data["greeting"]; got != "hello-v1" {
		t.Fatalf("rollback restored %q", got)
	}
	a = e.refresh(app.GetId())
	if a.GetSyncStatus() != "out_of_sync" || a.GetRevision() != second {
		t.Fatalf("after a rollback the source's head is out of sync again: %s %s", a.GetSyncStatus(), a.GetRevision())
	}

	// Resources SDI did not create: one beside the application, one that a
	// new commit declares with the same name.
	hand := &corev1.ConfigMap{ObjectMeta: metav1.ObjectMeta{Name: "hand-made", Namespace: ns}, Data: map[string]string{"by": "hand"}}
	if _, err := e.kube.CoreV1().ConfigMaps(ns).Create(context.Background(), hand, metav1.CreateOptions{}); err != nil {
		t.Fatal(err)
	}
	pre := &corev1.ConfigMap{ObjectMeta: metav1.ObjectMeta{Name: "preexisting", Namespace: ns}, Data: map[string]string{"owner": "human"}}
	if _, err := e.kube.CoreV1().ConfigMaps(ns).Create(context.Background(), pre, metav1.CreateOptions{}); err != nil {
		t.Fatal(err)
	}
	third := e.github.commit(func(files map[string][]byte) {
		delete(files, "directory/configmap.yaml")
		files["directory/preexisting.yaml"] = []byte("apiVersion: v1\nkind: ConfigMap\nmetadata:\n  name: preexisting\ndata:\n  owner: sdi\n")
	})
	a = e.refresh(app.GetId())
	if a.GetRevision() != third || resourceOf(a, "ConfigMap", "settings").GetSyncStatus() != "extraneous" ||
		resourceOf(a, "ConfigMap", "preexisting").GetSyncStatus() != "out_of_sync" || resourceOf(a, "ConfigMap", "hand-made") != nil {
		t.Fatalf("third commit: %v", a.GetResources())
	}
	if !strings.Contains(strings.Join(a.GetConditions(), " "), "was not created by this application") {
		t.Fatalf("a foreign resource is a condition: %v", a.GetConditions())
	}
	res = e.sync(&gitopsv1.SyncApplicationRequest{Id: app.GetId()})
	if res.GetPhase() != "failed" || resultOf(res, "ConfigMap", "settings").GetStatus() != "prune_skipped" ||
		resultOf(res, "ConfigMap", "preexisting").GetStatus() != "failed" {
		t.Fatalf("sync without prune: %s", describeResults(res))
	}
	e.configMap(ns, "settings")
	if got := e.configMap(ns, "preexisting"); got.Data["owner"] != "human" || got.Annotations[tracking.Annotation] != "" {
		t.Fatalf("a resource SDI did not create was changed: %v %v", got.Data, got.Annotations)
	}
	res = e.sync(&gitopsv1.SyncApplicationRequest{Id: app.GetId(), Prune: true})
	if resultOf(res, "ConfigMap", "settings").GetStatus() != "pruned" {
		t.Fatalf("sync with prune: %s", describeResults(res))
	}
	e.gone(ns, "ConfigMap", "settings", func() error {
		_, err := e.kube.CoreV1().ConfigMaps(ns).Get(context.Background(), "settings", metav1.GetOptions{})
		return err
	})
	if e.configMap(ns, "hand-made").Data["by"] != "hand" || e.configMap(ns, "preexisting").Data["owner"] != "human" {
		t.Fatal("prune touched resources without the tracking annotation")
	}

	// Secrets are masked in manifests and diffs.
	e.github.commit(func(files map[string][]byte) {
		delete(files, "directory/preexisting.yaml")
		files["directory/configmap.yaml"] = []byte("apiVersion: v1\nkind: ConfigMap\nmetadata:\n  name: settings\ndata:\n  greeting: hello-v4\n")
		files["directory/secret.yaml"] = []byte("apiVersion: v1\nkind: Secret\nmetadata:\n  name: db\ntype: Opaque\ndata:\n  password: aHVudGVyMg==\nstringData:\n  token: plain-token-value\n")
	})
	manifests, err = e.client.GetManifests(e.as(principal.Viewer), &gitopsv1.GetManifestsRequest{Id: app.GetId()})
	if err != nil {
		t.Fatal(err)
	}
	all := strings.Join(manifests.GetManifests(), "\n")
	if strings.Contains(all, "aHVudGVyMg==") || strings.Contains(all, "plain-token-value") || !strings.Contains(all, "++++++++") {
		t.Fatalf("Secret values leaked in manifests:\n%s", all)
	}
	res = e.sync(&gitopsv1.SyncApplicationRequest{Id: app.GetId(), Prune: true})
	if res.GetPhase() != "succeeded" {
		t.Fatalf("sync with a secret: %s", describeResults(res))
	}
	diff, err = e.client.DiffApplication(e.as(principal.Viewer), &gitopsv1.DiffApplicationRequest{Id: app.GetId()})
	if err != nil {
		t.Fatal(err)
	}
	for _, d := range diff.GetResources() {
		text := d.GetLive() + d.GetTarget() + d.GetDiff()
		if strings.Contains(text, "aHVudGVyMg==") || strings.Contains(text, "plain-token-value") || strings.Contains(text, "cGxhaW4tdG9rZW4tdmFsdWU=") {
			t.Fatalf("Secret values leaked in a diff of %s:\n%s", d.GetKind(), text)
		}
		if d.GetKind() == "Secret" && (!strings.Contains(d.GetLive(), "++++++++") || d.GetSyncStatus() != "synced") {
			t.Fatalf("the live Secret is masked and synced: %s\n%s", d.GetSyncStatus(), d.GetLive())
		}
	}
	e.waitHealthy(app.GetId())

	// Automation: self-heal undoes a manual edit; a new commit is synced on
	// its own.
	_, err = e.client.UpsertApplication(e.as(principal.Admin), &gitopsv1.UpsertApplicationRequest{Application: &gitopsv1.Application{
		Id: app.GetId(), ProjectId: e.projectID, Name: app.GetName(), ClusterId: c.GetId(), Namespace: ns,
		Source:     &gitopsv1.ApplicationSource{Path: "directory"},
		SyncPolicy: &gitopsv1.SyncPolicy{Automated: true, SelfHeal: true, Prune: true, CreateNamespace: true},
	}})
	if err != nil {
		t.Fatal(err)
	}
	edited := e.configMap(ns, "settings")
	edited.Data["greeting"] = "edited-by-hand"
	if _, err := e.kube.CoreV1().ConfigMaps(ns).Update(context.Background(), edited, metav1.UpdateOptions{}); err != nil {
		t.Fatal(err)
	}
	ref := gitops.Ref(e.workspace, mustUUID(t, app.GetId()))
	e.eventually(time.Minute, "self-heal", func() (bool, string) {
		e.reconciler.ReconcileOne(context.Background(), ref)
		got := e.configMap(ns, "settings").Data["greeting"]
		return got == "hello-v4", got
	})
	history, _ = e.client.ListHistory(e.as(principal.Viewer), &gitopsv1.ListHistoryRequest{Id: app.GetId()})
	if !history.GetEntries()[0].GetAutomated() || history.GetEntries()[0].GetInitiatedBy() != principal.SystemUser {
		t.Fatalf("the self-heal is an automated history entry: %v", history.GetEntries()[0])
	}
	e.eventually(10*time.Second, "auto_sync audit", func() (bool, string) {
		audits := e.identity.audited("application.auto_sync")
		return len(audits) > 0 && audits[0].GetUserId() == principal.SystemUser, fmt.Sprint(len(audits))
	})
	fifth := e.github.commit(func(files map[string][]byte) {
		files["directory/configmap.yaml"] = []byte("apiVersion: v1\nkind: ConfigMap\nmetadata:\n  name: settings\ndata:\n  greeting: hello-v5\n")
	})
	e.eventually(time.Minute, "automated sync of a new commit", func() (bool, string) {
		e.reconciler.ReconcileOne(context.Background(), ref)
		got := e.configMap(ns, "settings").Data["greeting"]
		return got == "hello-v5", got
	})
	a = e.refresh(app.GetId())
	if a.GetSyncedRevision() != fifth {
		t.Fatalf("synced revision %s, want %s", a.GetSyncedRevision(), fifth)
	}
	// Rollback is refused while automation would undo it.
	if _, err := e.client.RollbackApplication(e.as(principal.Approver), &gitopsv1.RollbackApplicationRequest{Id: app.GetId(), HistoryId: history.GetEntries()[0].GetId()}); status.Code(err) != codes.FailedPrecondition {
		t.Fatalf("rollback with automation on: %v", err)
	}

	// Cascade delete removes what the application created, nothing else.
	del, err := e.client.DeleteApplication(e.as(principal.Admin), &gitopsv1.DeleteApplicationRequest{Id: app.GetId(), Cascade: true})
	if err != nil || del.GetDeletedResources() != 4 {
		t.Fatalf("cascade delete: %v %v", del, err)
	}
	e.gone(ns, "Deployment", "web", func() error {
		_, err := e.kube.AppsV1().Deployments(ns).Get(context.Background(), "web", metav1.GetOptions{})
		return err
	})
	e.gone(ns, "Secret", "db", func() error {
		_, err := e.kube.CoreV1().Secrets(ns).Get(context.Background(), "db", metav1.GetOptions{})
		return err
	})
	if e.configMap(ns, "hand-made").Data["by"] != "hand" || e.configMap(ns, "preexisting").Data["owner"] != "human" {
		t.Fatal("a cascade delete removed resources SDI did not create")
	}
	if _, err := e.client.GetApplication(e.as(principal.Viewer), &gitopsv1.GetApplicationRequest{Id: app.GetId()}); status.Code(err) != codes.NotFound {
		t.Fatalf("deleted application: %v", err)
	}
}

func mustUUID(t *testing.T, s string) uuid.UUID {
	t.Helper()
	id, err := uuid.Parse(s)
	if err != nil {
		t.Fatalf("not an id: %q", s)
	}
	return id
}

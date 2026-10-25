package store_test

import (
	"context"
	"crypto/rand"
	"encoding/hex"
	"encoding/json"
	"errors"
	"os"
	"sync"
	"testing"
	"time"

	"github.com/google/uuid"
	"github.com/jackc/pgx/v5"

	"github.com/f8fvwgzc/software-delivery-intelligence/services/gitops/internal/config"
	"github.com/f8fvwgzc/software-delivery-intelligence/services/gitops/internal/db"
	"github.com/f8fvwgzc/software-delivery-intelligence/services/gitops/internal/store"
)

// open connects to DATABASE_URL in a schema of its own, dropped at the end.
func open(t *testing.T) *store.Store {
	t.Helper()
	url := os.Getenv("DATABASE_URL")
	if url == "" {
		t.Skip("DATABASE_URL is not set")
	}
	b := make([]byte, 6)
	_, _ = rand.Read(b)
	schema := "gitops_test_" + hex.EncodeToString(b)
	ctx := context.Background()
	pool, err := db.Connect(ctx, url, schema, config.DefaultPool())
	if err != nil {
		t.Fatalf("connect: %v", err)
	}
	t.Cleanup(func() {
		pool.Close()
		conn, err := pgx.Connect(context.Background(), url)
		if err != nil {
			t.Logf("could not drop %s: %v", schema, err)
			return
		}
		defer conn.Close(context.Background())
		if _, err := conn.Exec(context.Background(), "DROP SCHEMA "+schema+" CASCADE"); err != nil {
			t.Logf("could not drop %s: %v", schema, err)
		}
	})
	// Migrating twice is a no-op.
	if err := db.Migrate(ctx, pool); err != nil {
		t.Fatalf("second migrate: %v", err)
	}
	return store.New(pool)
}

func cluster(ws uuid.UUID, name string) store.Cluster {
	return store.Cluster{ID: uuid.Must(uuid.NewV7()), WorkspaceID: ws, Name: name, KubeconfigSecret: "KUBECONFIG",
		DefaultNamespace: "default", Namespaces: []string{}, Status: "unknown"}
}

func application(ws, clusterID uuid.UUID, name string) store.Application {
	return store.Application{ID: uuid.Must(uuid.NewV7()), WorkspaceID: ws, ProjectID: "p1", Name: name, ClusterID: clusterID,
		Namespace: "shop", Source: store.Source{Path: "deploy"}, Policy: store.SyncPolicy{Automated: true}}
}

func TestClustersAreScopedToTheirWorkspace(t *testing.T) {
	s := open(t)
	ctx := context.Background()
	ws, other := uuid.New(), uuid.New()
	c, err := s.UpsertCluster(ctx, cluster(ws, "prod"))
	if err != nil {
		t.Fatal(err)
	}
	if _, err := s.UpsertCluster(ctx, cluster(ws, "prod")); !errors.Is(err, store.ErrConflict) {
		t.Fatalf("a second cluster with the same name: %v", err)
	}
	if _, err := s.UpsertCluster(ctx, cluster(other, "prod")); err != nil {
		t.Fatalf("another workspace may use the name: %v", err)
	}
	if _, err := s.GetCluster(ctx, other, c.ID); !errors.Is(err, store.ErrNotFound) {
		t.Fatalf("another workspace's cluster must be invisible: %v", err)
	}
	// Updating by id from another workspace changes nothing.
	hijack := c
	hijack.WorkspaceID = other
	hijack.Name = "stolen"
	if _, err := s.UpsertCluster(ctx, hijack); !errors.Is(err, store.ErrNotFound) {
		t.Fatalf("cross-workspace update: %v", err)
	}
	got, _ := s.GetCluster(ctx, ws, c.ID)
	if got.Name != "prod" {
		t.Fatalf("the cluster was renamed from another workspace: %s", got.Name)
	}
	now := time.Now().UTC()
	c.Status, c.ServerVersion, c.CheckedAt, c.Namespaces = "connected", "v1.31.0", &now, []string{"a", "b"}
	updated, err := s.UpsertCluster(ctx, c)
	if err != nil || updated.Status != "connected" || len(updated.Namespaces) != 2 || !updated.UpdatedAt.After(c.UpdatedAt) {
		t.Fatalf("update: %+v %v", updated, err)
	}
	if updated.Fingerprint() == got.Fingerprint() {
		t.Fatal("an update changes the fingerprint (the cluster cache is rebuilt)")
	}
	list, err := s.ListClusters(ctx, ws)
	if err != nil || len(list) != 1 {
		t.Fatalf("list: %d %v", len(list), err)
	}
}

func TestApplicationsStatusOperationsAndHistory(t *testing.T) {
	s := open(t)
	ctx := context.Background()
	ws, other := uuid.New(), uuid.New()
	c, _ := s.UpsertCluster(ctx, cluster(ws, "prod"))
	foreignCluster, _ := s.UpsertCluster(ctx, cluster(other, "theirs"))

	// An application may not point at another workspace's cluster.
	if err := s.InsertApplication(ctx, application(ws, foreignCluster.ID, "web")); !errors.Is(err, store.ErrInUse) {
		t.Fatalf("cross-workspace cluster reference: %v", err)
	}
	a := application(ws, c.ID, "web")
	a.Source.HelmParameters = map[string]string{"image.tag": "1"}
	if err := s.InsertApplication(ctx, a); err != nil {
		t.Fatal(err)
	}
	if err := s.InsertApplication(ctx, application(ws, c.ID, "web")); !errors.Is(err, store.ErrConflict) {
		t.Fatalf("duplicate name in a project: %v", err)
	}
	got, err := s.GetApplication(ctx, ws, a.ID)
	if err != nil || got.ClusterName != "prod" || got.SyncStatus != "unknown" || got.Source.HelmParameters["image.tag"] != "1" || !got.Policy.Automated {
		t.Fatalf("get: %+v %v", got, err)
	}
	if _, err := s.GetApplication(ctx, other, a.ID); !errors.Is(err, store.ErrNotFound) {
		t.Fatalf("another workspace's application: %v", err)
	}
	if err := s.DeleteCluster(ctx, ws, c.ID); !errors.Is(err, store.ErrInUse) {
		t.Fatalf("a cluster in use cannot go: %v", err)
	}

	st := store.Status{SyncStatus: "out_of_sync", HealthStatus: "missing", Revision: "abc", DesiredDigest: "d1",
		Conditions: []string{"one"}, Resources: []store.ResourceStatus{{Kind: "ConfigMap", Name: "x", SyncStatus: "missing"}},
		ResourceCount: 1, OutOfSyncCount: 1, ReconciledAt: time.Now().UTC()}
	if err := s.SaveStatus(ctx, ws, a.ID, st); err != nil {
		t.Fatal(err)
	}
	if err := s.SaveConditions(ctx, ws, a.ID, []string{"cluster unreachable"}); err != nil {
		t.Fatal(err)
	}
	got, _ = s.GetApplication(ctx, ws, a.ID)
	if got.SyncStatus != "out_of_sync" || len(got.Resources) != 1 || got.Conditions[0] != "cluster unreachable" || got.DesiredDigest != "d1" {
		t.Fatalf("a failed comparison keeps the statuses and replaces the conditions: %+v", got)
	}
	list, _ := s.ListApplications(ctx, ws, "p1")
	if len(list) != 1 || list[0].Resources != nil {
		t.Fatalf("summaries carry no resource tree: %+v", list)
	}

	// One operation at a time, across callers.
	op := store.Operation{Kind: "sync", Phase: "running", InitiatedBy: "u1", StartedAt: time.Now().UTC()}
	var wins int
	var mu sync.Mutex
	var wg sync.WaitGroup
	for i := 0; i < 8; i++ {
		wg.Add(1)
		go func() {
			defer wg.Done()
			ok, err := s.ClaimOperation(ctx, ws, a.ID, op, time.Minute)
			if err != nil {
				t.Error(err)
			}
			if ok {
				mu.Lock()
				wins++
				mu.Unlock()
			}
		}()
	}
	wg.Wait()
	if wins != 1 {
		t.Fatalf("%d operations claimed the application at once", wins)
	}
	got, _ = s.GetApplication(ctx, ws, a.ID)
	if !got.OperationRunning(time.Now()) || got.Operation.Phase != "running" {
		t.Fatalf("the claim is visible: %+v", got.Operation)
	}
	finished := time.Now().UTC()
	op.Phase, op.FinishedAt = "succeeded", &finished
	if err := s.FinishOperation(ctx, ws, a.ID, &op, &[2]string{"abc", "d1"}); err != nil {
		t.Fatal(err)
	}
	got, _ = s.GetApplication(ctx, ws, a.ID)
	if got.OperationRunning(time.Now()) || got.SyncedRevision != "abc" || got.SyncedDigest != "d1" || got.Operation.Phase != "succeeded" {
		t.Fatalf("finished: %+v", got)
	}
	// An expired lease (a replica that died) can be taken over.
	if ok, _ := s.ClaimOperation(ctx, ws, a.ID, op, time.Millisecond); !ok {
		t.Fatal("claim after finish")
	}
	time.Sleep(20 * time.Millisecond)
	if ok, _ := s.ClaimOperation(ctx, ws, a.ID, op, time.Minute); !ok {
		t.Fatal("an expired lease must not block forever")
	}
	if ok, _ := s.ClaimOperation(ctx, other, a.ID, op, time.Minute); ok {
		t.Fatal("another workspace claimed the application")
	}

	next := time.Now().Add(time.Minute).UTC()
	if err := s.SaveAutomation(ctx, ws, a.ID, "d2", 2, &next); err != nil {
		t.Fatal(err)
	}
	got, _ = s.GetApplication(ctx, ws, a.ID)
	if got.AutoDigest != "d2" || got.AutoAttempts != 2 || got.AutoNextAt == nil {
		t.Fatalf("automation state: %+v", got)
	}

	// History keeps the newest 50.
	start := time.Now().UTC().Add(-time.Hour)
	for i := 0; i < store.MaxHistory+5; i++ {
		at := start.Add(time.Duration(i) * time.Second)
		if err := s.InsertHistory(ctx, store.HistoryEntry{ID: uuid.Must(uuid.NewV7()), WorkspaceID: ws, ApplicationID: a.ID,
			Kind: "sync", Revision: "r", Source: a.Source, Phase: "succeeded", InitiatedBy: "u1", StartedAt: at, FinishedAt: at}); err != nil {
			t.Fatal(err)
		}
	}
	entries, err := s.ListHistory(ctx, ws, a.ID, 100)
	if err != nil || len(entries) != store.MaxHistory {
		t.Fatalf("history holds %d: %v", len(entries), err)
	}
	if !entries[0].StartedAt.After(entries[len(entries)-1].StartedAt) {
		t.Fatal("newest first")
	}
	if _, err := s.GetHistory(ctx, other, a.ID, entries[0].ID); !errors.Is(err, store.ErrNotFound) {
		t.Fatalf("another workspace's history: %v", err)
	}
	if h, err := s.GetHistory(ctx, ws, a.ID, entries[0].ID); err != nil || h.Source.Path != "deploy" {
		t.Fatalf("history source: %+v %v", h, err)
	}

	refs, err := s.AllApplications(ctx)
	if err != nil || len(refs) != 1 || refs[0].WorkspaceID != ws {
		t.Fatalf("all applications: %+v %v", refs, err)
	}
}

func TestExportAndDeleteCoverEveryTable(t *testing.T) {
	s := open(t)
	ctx := context.Background()
	ws, other := uuid.New(), uuid.New()
	c, _ := s.UpsertCluster(ctx, cluster(ws, "prod"))
	a := application(ws, c.ID, "web")
	_ = s.InsertApplication(ctx, a)
	now := time.Now().UTC()
	_ = s.InsertHistory(ctx, store.HistoryEntry{ID: uuid.Must(uuid.NewV7()), WorkspaceID: ws, ApplicationID: a.ID, Kind: "sync",
		Revision: "r", Source: a.Source, Phase: "succeeded", InitiatedBy: "u1", StartedAt: now, FinishedAt: now})
	oc, _ := s.UpsertCluster(ctx, cluster(other, "theirs"))
	_ = s.InsertApplication(ctx, application(other, oc.ID, "theirs"))

	var entities []string
	err := s.ExportWorkspace(ctx, ws, func(entity, raw string) error {
		var row map[string]any
		if err := json.Unmarshal([]byte(raw), &row); err != nil {
			t.Fatalf("not JSON: %s", raw)
		}
		if row["workspace_id"] != ws.String() {
			t.Fatalf("another workspace's row exported: %s", raw)
		}
		entities = append(entities, entity)
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
	if len(entities) != 3 || entities[0] != "cluster" || entities[1] != "application" || entities[2] != "history" {
		t.Fatalf("parents before children, every table: %v", entities)
	}
	deleted, err := s.DeleteWorkspace(ctx, ws)
	if err != nil {
		t.Fatal(err)
	}
	if deleted["cluster"] != 1 || deleted["application"] != 1 || deleted["history"] != 1 {
		t.Fatalf("deleted %v", deleted)
	}
	if list, _ := s.ListApplications(ctx, other, ""); len(list) != 1 {
		t.Fatal("another workspace's data went too")
	}
	if list, _ := s.ListClusters(ctx, ws); len(list) != 0 {
		t.Fatal("rows survived the delete")
	}
}

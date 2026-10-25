package server

import (
	"context"
	"io"
	"log/slog"
	"sync"
	"testing"
	"time"

	"github.com/google/uuid"
	"google.golang.org/grpc"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/metadata"
	"google.golang.org/grpc/status"

	commonv1 "github.com/f8fvwgzc/software-delivery-intelligence/services/gitops/gen/sdi/common/v1"
	gitopsv1 "github.com/f8fvwgzc/software-delivery-intelligence/services/gitops/gen/sdi/gitops/v1"
	"github.com/f8fvwgzc/software-delivery-intelligence/services/gitops/internal/events"
	"github.com/f8fvwgzc/software-delivery-intelligence/services/gitops/internal/gitops"
	"github.com/f8fvwgzc/software-delivery-intelligence/services/gitops/internal/principal"
	"github.com/f8fvwgzc/software-delivery-intelligence/services/gitops/internal/store"
)

func as(role principal.Role) context.Context {
	return principal.Principal{UserID: "u1", WorkspaceID: uuid.New(), Role: role}.Incoming(context.Background())
}

// unaryCalls is every unary RPC with the least role it requires (§24.1).
func unaryCalls(s *Server) map[string]struct {
	min  principal.Role
	call func(context.Context) error
} {
	id := uuid.NewString()
	wrap := func(_ any, err error) error { return err }
	return map[string]struct {
		min  principal.Role
		call func(context.Context) error
	}{
		"ListClusters":     {principal.Viewer, func(c context.Context) error { return wrap(s.ListClusters(c, &gitopsv1.ListClustersRequest{})) }},
		"UpsertCluster":    {principal.Admin, func(c context.Context) error { return wrap(s.UpsertCluster(c, &gitopsv1.UpsertClusterRequest{})) }},
		"DeleteCluster":    {principal.Admin, func(c context.Context) error { return wrap(s.DeleteCluster(c, &gitopsv1.DeleteClusterRequest{Id: id})) }},
		"ListApplications": {principal.Viewer, func(c context.Context) error { return wrap(s.ListApplications(c, &gitopsv1.ListApplicationsRequest{})) }},
		"GetApplication": {principal.Viewer, func(c context.Context) error {
			return wrap(s.GetApplication(c, &gitopsv1.GetApplicationRequest{Id: id}))
		}},
		"UpsertApplication": {principal.Admin, func(c context.Context) error {
			return wrap(s.UpsertApplication(c, &gitopsv1.UpsertApplicationRequest{}))
		}},
		"DeleteApplication": {principal.Admin, func(c context.Context) error {
			return wrap(s.DeleteApplication(c, &gitopsv1.DeleteApplicationRequest{Id: id}))
		}},
		"RefreshApplication": {principal.Member, func(c context.Context) error {
			return wrap(s.RefreshApplication(c, &gitopsv1.RefreshApplicationRequest{Id: id}))
		}},
		"GetManifests": {principal.Viewer, func(c context.Context) error { return wrap(s.GetManifests(c, &gitopsv1.GetManifestsRequest{Id: id})) }},
		"DiffApplication": {principal.Viewer, func(c context.Context) error {
			return wrap(s.DiffApplication(c, &gitopsv1.DiffApplicationRequest{Id: id}))
		}},
		"SyncApplication": {principal.Approver, func(c context.Context) error {
			return wrap(s.SyncApplication(c, &gitopsv1.SyncApplicationRequest{Id: id}))
		}},
		"RollbackApplication": {principal.Approver, func(c context.Context) error {
			return wrap(s.RollbackApplication(c, &gitopsv1.RollbackApplicationRequest{Id: id}))
		}},
		"ListHistory": {principal.Viewer, func(c context.Context) error { return wrap(s.ListHistory(c, &gitopsv1.ListHistoryRequest{Id: id})) }},
		"DeleteWorkspaceData": {principal.Owner, func(c context.Context) error {
			return wrap(s.DeleteWorkspaceData(c, &gitopsv1.DeleteWorkspaceDataRequest{}))
		}},
	}
}

func TestEveryRPCChecksTheCallersRole(t *testing.T) {
	s := New(gitops.New(gitops.Options{Log: slog.New(slog.NewTextHandler(io.Discard, nil))}), slog.New(slog.NewTextHandler(io.Discard, nil)))
	for name, c := range unaryCalls(s) {
		if c.min > principal.Viewer {
			if err := c.call(as(c.min - 1)); status.Code(err) != codes.PermissionDenied {
				t.Errorf("%s as %s: %v, want PERMISSION_DENIED", name, c.min-1, err)
			}
		}
		if err := c.call(context.Background()); status.Code(err) != codes.Unauthenticated {
			t.Errorf("%s without metadata: %v, want UNAUTHENTICATED", name, err)
		}
	}
	stream := &fakeStream[commonv1.PlatformEvent]{ctx: context.Background()}
	if err := s.WatchEvents(&commonv1.WatchEventsRequest{}, stream); status.Code(err) != codes.Unauthenticated {
		t.Errorf("WatchEvents without metadata: %v", err)
	}
	export := &fakeStream[gitopsv1.WorkspaceDataRecord]{ctx: as(principal.Approver)}
	if err := s.ExportWorkspaceData(&gitopsv1.ExportWorkspaceDataRequest{}, export); status.Code(err) != codes.PermissionDenied {
		t.Errorf("ExportWorkspaceData as approver: %v", err)
	}
}

func TestWorkspaceDataMustNameTheCallersWorkspace(t *testing.T) {
	p := principal.Principal{UserID: "u", WorkspaceID: uuid.New(), Role: principal.Owner}
	if err := checkWorkspace(p, ""); status.Code(err) != codes.InvalidArgument {
		t.Errorf("empty: %v", err)
	}
	if err := checkWorkspace(p, uuid.NewString()); status.Code(err) != codes.PermissionDenied {
		t.Errorf("another workspace: %v", err)
	}
	if err := checkWorkspace(p, p.WorkspaceID.String()); err != nil {
		t.Errorf("own workspace: %v", err)
	}
}

type fakeStream[T any] struct {
	grpc.ServerStream
	ctx  context.Context
	mu   sync.Mutex
	sent []*T
}

func (f *fakeStream[T]) Context() context.Context { return f.ctx }
func (f *fakeStream[T]) Send(m *T) error {
	f.mu.Lock()
	defer f.mu.Unlock()
	f.sent = append(f.sent, m)
	return nil
}
func (f *fakeStream[T]) count() int {
	f.mu.Lock()
	defer f.mu.Unlock()
	return len(f.sent)
}
func (f *fakeStream[T]) all() []*T {
	f.mu.Lock()
	defer f.mu.Unlock()
	return append([]*T(nil), f.sent...)
}
func (f *fakeStream[T]) SetHeader(metadata.MD) error {
	return nil
}

func TestClusterValidation(t *testing.T) {
	good := store.Cluster{Name: "prod-eu", KubeconfigSecret: "PROD_KUBECONFIG", Namespaces: []string{"b", "a", "b"}}
	c, err := cleanCluster(good)
	if err != nil {
		t.Fatal(err)
	}
	if c.DefaultNamespace != "default" || len(c.Namespaces) != 2 || c.Namespaces[0] != "a" {
		t.Fatalf("normalised %+v", c)
	}
	for name, bad := range map[string]store.Cluster{
		"name":      {Name: "Prod EU", KubeconfigSecret: "K8S"},
		"secret":    {Name: "prod", KubeconfigSecret: "kubeconfig"},
		"namespace": {Name: "prod", KubeconfigSecret: "K8S", DefaultNamespace: "Bad_NS"},
		"listed":    {Name: "prod", KubeconfigSecret: "K8S", Namespaces: []string{"ok", "not ok"}},
		"context":   {Name: "prod", KubeconfigSecret: "K8S", Context: "a\nb"},
	} {
		if _, err := cleanCluster(bad); status.Code(err) != codes.InvalidArgument {
			t.Errorf("%s: %v", name, err)
		}
	}
}

func TestApplicationValidation(t *testing.T) {
	cluster := store.Cluster{Name: "prod", DefaultNamespace: "apps"}
	a, err := cleanApplication(store.Application{Name: "web", ProjectID: "p1", Source: store.Source{Path: "./deploy/", Type: "kustomize",
		KustomizeImages: []string{" web=registry/web:1 "}}}, cluster)
	if err != nil {
		t.Fatal(err)
	}
	if a.Namespace != "apps" || a.Source.Path != "deploy" || a.Source.KustomizeImages[0] != "web=registry/web:1" {
		t.Fatalf("normalised %+v", a)
	}
	for name, bad := range map[string]store.Application{
		"name":        {Name: "Web App", ProjectID: "p"},
		"long name":   {Name: "a-name-that-is-far-too-long-for-a-helm-release-name-really", ProjectID: "p"},
		"project":     {Name: "web"},
		"namespace":   {Name: "web", ProjectID: "p", Namespace: "No"},
		"repo":        {Name: "web", ProjectID: "p", Source: store.Source{Repo: "not a repo"}},
		"path":        {Name: "web", ProjectID: "p", Source: store.Source{Path: "../../etc"}},
		"type":        {Name: "web", ProjectID: "p", Source: store.Source{Type: "jsonnet"}},
		"values file": {Name: "web", ProjectID: "p", Source: store.Source{HelmValueFiles: []string{"https://x/values.yaml"}}},
		"image":       {Name: "web", ProjectID: "p", Source: store.Source{KustomizeImages: []string{"nope"}}},
		"release":     {Name: "web", ProjectID: "p", Source: store.Source{HelmReleaseName: "Bad_Release"}},
		"retry":       {Name: "web", ProjectID: "p", Policy: store.SyncPolicy{RetryLimit: 11}},
		"revision":    {Name: "web", ProjectID: "p", Source: store.Source{Revision: "main branch"}},
	} {
		if _, err := cleanApplication(bad, cluster); status.Code(err) != codes.InvalidArgument {
			t.Errorf("%s: %v", name, err)
		}
	}
	namespaced := store.Cluster{Name: "shared", DefaultNamespace: "team-a", Namespaces: []string{"team-a"}}
	if _, err := cleanApplication(store.Application{Name: "web", ProjectID: "p", Namespace: "team-b"}, namespaced); status.Code(err) != codes.InvalidArgument {
		t.Errorf("a namespace the cluster does not manage: %v", err)
	}
}

func TestWatchEventsStreamsTheWorkspacesEventsUntilItEnds(t *testing.T) {
	leaving, leave := context.WithCancel(context.Background())
	svc := gitops.New(gitops.Options{Log: slog.New(slog.NewTextHandler(io.Discard, nil)), Events: events.NewHub()})
	s := New(svc, slog.New(slog.NewTextHandler(io.Discard, nil)))
	s.Leaving = leaving
	p := principal.Principal{UserID: "u", WorkspaceID: uuid.New(), Role: principal.Viewer}
	stream := &fakeStream[commonv1.PlatformEvent]{ctx: p.Incoming(context.Background())}
	done := make(chan error, 1)
	go func() { done <- s.WatchEvents(&commonv1.WatchEventsRequest{}, stream) }()
	app := uuid.New()
	deadline := time.Now().Add(5 * time.Second)
	for {
		svc.Events.Application(uuid.New(), uuid.New(), "other", "updated", "synced", nil)
		svc.Events.Application(p.WorkspaceID, app, "p1", "updated", "out_of_sync", nil)
		time.Sleep(20 * time.Millisecond)
		if stream.count() > 0 || time.Now().After(deadline) {
			break
		}
	}
	leave()
	if err := <-done; err != nil {
		t.Fatalf("the stream ends cleanly on shutdown: %v", err)
	}
	for _, e := range stream.all() {
		if e.GetData()["workspace_id"] != p.WorkspaceID.String() || e.GetEntityId() != app.String() {
			t.Fatalf("an event of another workspace was streamed: %+v", e)
		}
	}
	if stream.count() == 0 {
		t.Fatal("no event was streamed")
	}
}

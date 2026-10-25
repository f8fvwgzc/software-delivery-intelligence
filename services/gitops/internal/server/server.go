// Package server is the GitopsService gRPC API (proto/sdi/gitops/v1):
// the principal from metadata on every call, the role checks of CONTRACT.md
// §24.1, tenancy on every query, and the conversions to the wire types.
package server

import (
	"context"
	"errors"
	"fmt"
	"log/slog"
	"strings"
	"time"

	"github.com/google/uuid"
	"google.golang.org/grpc"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/status"

	commonv1 "github.com/f8fvwgzc/software-delivery-intelligence/services/gitops/gen/sdi/common/v1"
	gitopsv1 "github.com/f8fvwgzc/software-delivery-intelligence/services/gitops/gen/sdi/gitops/v1"
	"github.com/f8fvwgzc/software-delivery-intelligence/services/gitops/internal/clusters"
	"github.com/f8fvwgzc/software-delivery-intelligence/services/gitops/internal/gitops"
	"github.com/f8fvwgzc/software-delivery-intelligence/services/gitops/internal/principal"
	"github.com/f8fvwgzc/software-delivery-intelligence/services/gitops/internal/store"
)

// Workspace bounds, so every list answer stays bounded too.
const (
	MaxClustersPerWorkspace     = 200
	MaxApplicationsPerWorkspace = 2000
)

// Server implements GitopsServiceServer.
type Server struct {
	gitopsv1.UnimplementedGitopsServiceServer
	svc *gitops.Service
	log *slog.Logger
	// ForgetWorkspace drops what is cached in memory for a workspace whose
	// data was deleted (GitHub tokens).
	ForgetWorkspace func(uuid.UUID)
	// Leaving ends with the shutdown signal: open event streams end then.
	Leaving context.Context
}

// New wraps the controller.
func New(svc *gitops.Service, log *slog.Logger) *Server {
	return &Server{svc: svc, log: log, Leaving: svc.Shutdown}
}

// caller reads the principal and checks its role.
func caller(ctx context.Context, min principal.Role) (principal.Principal, error) {
	p, err := principal.FromIncoming(ctx)
	if err != nil {
		return p, err
	}
	return p, p.Require(min)
}

func parseID(raw, field string) (uuid.UUID, error) {
	id, err := uuid.Parse(strings.TrimSpace(raw))
	if err != nil {
		return uuid.Nil, invalid("%s must be a valid id", field)
	}
	return id, nil
}

// storeErr maps a store error to a status without leaking SQL details.
func (s *Server) storeErr(err error, what string) error {
	switch {
	case err == nil:
		return nil
	case errors.Is(err, store.ErrNotFound):
		return status.Errorf(codes.NotFound, "no such %s", what)
	case errors.Is(err, store.ErrConflict):
		return status.Errorf(codes.AlreadyExists, "a %s with that name already exists", what)
	case errors.Is(err, store.ErrInUse):
		return status.Errorf(codes.FailedPrecondition, "the %s is still in use", what)
	case errors.Is(err, context.Canceled), errors.Is(err, context.DeadlineExceeded):
		return status.FromContextError(err).Err()
	}
	if _, ok := status.FromError(err); ok && status.Code(err) != codes.Unknown {
		return err
	}
	s.log.Error("database error", "error", err)
	return status.Error(codes.Internal, "database error")
}

func (s *Server) app(ctx context.Context, p principal.Principal, raw string) (store.Application, error) {
	id, err := parseID(raw, "id")
	if err != nil {
		return store.Application{}, err
	}
	a, err := s.svc.Store.GetApplication(ctx, p.WorkspaceID, id)
	return a, s.storeErr(err, "application")
}

// --- Clusters ---------------------------------------------------------------------

func (s *Server) ListClusters(ctx context.Context, _ *gitopsv1.ListClustersRequest) (*gitopsv1.ListClustersResponse, error) {
	p, err := caller(ctx, principal.Viewer)
	if err != nil {
		return nil, err
	}
	rows, err := s.svc.Store.ListClusters(ctx, p.WorkspaceID)
	if err != nil {
		return nil, s.storeErr(err, "cluster")
	}
	out := &gitopsv1.ListClustersResponse{}
	for _, c := range rows {
		out.Clusters = append(out.Clusters, clusterToProto(c))
	}
	return out, nil
}

func (s *Server) UpsertCluster(ctx context.Context, req *gitopsv1.UpsertClusterRequest) (*gitopsv1.Cluster, error) {
	p, err := caller(ctx, principal.Admin)
	if err != nil {
		return nil, err
	}
	in := req.GetCluster()
	if in == nil {
		return nil, invalid("cluster is required")
	}
	c := store.Cluster{
		WorkspaceID:      p.WorkspaceID,
		Name:             in.GetName(),
		KubeconfigSecret: in.GetKubeconfigSecret(),
		Context:          in.GetContext(),
		DefaultNamespace: in.GetDefaultNamespace(),
		Namespaces:       append([]string{}, in.GetNamespaces()...),
	}
	if strings.TrimSpace(in.GetId()) == "" {
		n, err := s.svc.Store.CountClusters(ctx, p.WorkspaceID)
		if err != nil {
			return nil, s.storeErr(err, "cluster")
		}
		if n >= MaxClustersPerWorkspace {
			return nil, status.Errorf(codes.ResourceExhausted, "a workspace holds at most %d clusters", MaxClustersPerWorkspace)
		}
		c.ID = uuid.Must(uuid.NewV7())
	} else {
		id, err := parseID(in.GetId(), "cluster.id")
		if err != nil {
			return nil, err
		}
		if _, err := s.svc.Store.GetCluster(ctx, p.WorkspaceID, id); err != nil {
			return nil, s.storeErr(err, "cluster")
		}
		c.ID = id
	}
	if c, err = cleanCluster(c); err != nil {
		return nil, err
	}
	// Reach it once, so a bad kubeconfig is reported when it is saved; an
	// unreachable cluster is still saved.
	checkCtx, cancel := context.WithTimeout(ctx, 20*time.Second)
	server, version, checkErr := s.svc.Clusters.Check(checkCtx, c)
	cancel()
	now := time.Now().UTC()
	c.Server, c.ServerVersion, c.CheckedAt = server, version, &now
	if checkErr != nil {
		c.Status, c.Message = "unreachable", sentenceOf(checkErr)
	} else {
		c.Status, c.Message = "connected", ""
	}
	saved, err := s.svc.Store.UpsertCluster(ctx, c)
	if err != nil {
		return nil, s.storeErr(err, "cluster")
	}
	s.svc.Clusters.Drop(saved.ID)
	s.svc.Audit.RecordAudit(p, "cluster.upsert", "cluster", saved.ID.String(), fmt.Sprintf("%s (%s)", saved.Name, saved.Status))
	full, err := s.svc.Store.GetCluster(ctx, p.WorkspaceID, saved.ID)
	if err != nil {
		return clusterToProto(saved), nil
	}
	return clusterToProto(full), nil
}

func sentenceOf(err error) string {
	var ce *clusters.Error
	if errors.As(err, &ce) {
		return ce.Message
	}
	return status.Convert(err).Message()
}

func (s *Server) DeleteCluster(ctx context.Context, req *gitopsv1.DeleteClusterRequest) (*gitopsv1.DeleteClusterResponse, error) {
	p, err := caller(ctx, principal.Admin)
	if err != nil {
		return nil, err
	}
	id, err := parseID(req.GetId(), "id")
	if err != nil {
		return nil, err
	}
	c, err := s.svc.Store.GetCluster(ctx, p.WorkspaceID, id)
	if err != nil {
		return nil, s.storeErr(err, "cluster")
	}
	if c.ApplicationCount > 0 {
		return nil, status.Errorf(codes.FailedPrecondition, "%d application(s) target this cluster; delete or move them first", c.ApplicationCount)
	}
	if err := s.svc.Store.DeleteCluster(ctx, p.WorkspaceID, id); err != nil {
		if errors.Is(err, store.ErrInUse) {
			return nil, status.Error(codes.FailedPrecondition, "applications target this cluster; delete or move them first")
		}
		return nil, s.storeErr(err, "cluster")
	}
	s.svc.Clusters.Drop(id)
	s.svc.Audit.RecordAudit(p, "cluster.delete", "cluster", id.String(), c.Name)
	return &gitopsv1.DeleteClusterResponse{}, nil
}

// --- Applications ---------------------------------------------------------------------

func (s *Server) ListApplications(ctx context.Context, req *gitopsv1.ListApplicationsRequest) (*gitopsv1.ListApplicationsResponse, error) {
	p, err := caller(ctx, principal.Viewer)
	if err != nil {
		return nil, err
	}
	rows, err := s.svc.Store.ListApplications(ctx, p.WorkspaceID, strings.TrimSpace(req.GetProjectId()))
	if err != nil {
		return nil, s.storeErr(err, "application")
	}
	out := &gitopsv1.ListApplicationsResponse{}
	for _, a := range rows {
		out.Applications = append(out.Applications, applicationToProto(a, false))
	}
	return out, nil
}

func (s *Server) GetApplication(ctx context.Context, req *gitopsv1.GetApplicationRequest) (*gitopsv1.Application, error) {
	p, err := caller(ctx, principal.Viewer)
	if err != nil {
		return nil, err
	}
	a, err := s.app(ctx, p, req.GetId())
	if err != nil {
		return nil, err
	}
	return applicationToProto(a, true), nil
}

func (s *Server) UpsertApplication(ctx context.Context, req *gitopsv1.UpsertApplicationRequest) (*gitopsv1.Application, error) {
	p, err := caller(ctx, principal.Admin)
	if err != nil {
		return nil, err
	}
	in := req.GetApplication()
	if in == nil {
		return nil, invalid("application is required")
	}
	clusterID, err := parseID(in.GetClusterId(), "application.cluster_id")
	if err != nil {
		return nil, err
	}
	cluster, err := s.svc.Store.GetCluster(ctx, p.WorkspaceID, clusterID)
	if err != nil {
		return nil, s.storeErr(err, "cluster")
	}
	a := store.Application{
		WorkspaceID: p.WorkspaceID,
		ProjectID:   in.GetProjectId(),
		Name:        in.GetName(),
		ClusterID:   clusterID,
		Namespace:   in.GetNamespace(),
		Source:      sourceFromProto(in.GetSource()),
		Policy:      policyFromProto(in.GetSyncPolicy()),
	}
	if a, err = cleanApplication(a, cluster); err != nil {
		return nil, err
	}
	creating := strings.TrimSpace(in.GetId()) == ""
	if creating {
		n, err := s.svc.Store.CountApplications(ctx, p.WorkspaceID)
		if err != nil {
			return nil, s.storeErr(err, "application")
		}
		if n >= MaxApplicationsPerWorkspace {
			return nil, status.Errorf(codes.ResourceExhausted, "a workspace holds at most %d applications", MaxApplicationsPerWorkspace)
		}
		a.ID = uuid.Must(uuid.NewV7())
	} else {
		existing, err := s.app(ctx, p, in.GetId())
		if err != nil {
			return nil, err
		}
		a.ID = existing.ID
		if existing.ProjectID != a.ProjectID {
			return nil, invalid("an application cannot move to another project")
		}
	}
	// The project must be one of the caller's workspace.
	if _, err := s.svc.Projects.GetProject(ctx, p, a.ProjectID); err != nil {
		if status.Code(err) == codes.NotFound {
			return nil, invalid("no such project")
		}
		return nil, status.Errorf(codes.Unavailable, "the project could not be checked: %s", status.Convert(err).Message())
	}
	if creating {
		err = s.svc.Store.InsertApplication(ctx, a)
	} else {
		err = s.svc.Store.UpdateApplicationSpec(ctx, a)
	}
	if err != nil {
		return nil, s.storeErr(err, "application")
	}
	action := "updated"
	if creating {
		action = "created"
	}
	detail := fmt.Sprintf("%s on %s/%s (automated: %t)", a.Name, cluster.Name, a.Namespace, a.Policy.Automated)
	s.svc.Audit.RecordAudit(p, "application.upsert", "application", a.ID.String(), detail)
	saved, err := s.svc.Store.GetApplication(ctx, p.WorkspaceID, a.ID)
	if err != nil {
		return nil, s.storeErr(err, "application")
	}
	s.svc.Events.Application(p.WorkspaceID, a.ID, a.ProjectID, action, saved.SyncStatus, map[string]string{"name": a.Name})
	s.svc.RefreshInBackground(p.WorkspaceID, a.ID)
	return applicationToProto(saved, true), nil
}

func (s *Server) DeleteApplication(ctx context.Context, req *gitopsv1.DeleteApplicationRequest) (*gitopsv1.DeleteApplicationResponse, error) {
	p, err := caller(ctx, principal.Admin)
	if err != nil {
		return nil, err
	}
	a, err := s.app(ctx, p, req.GetId())
	if err != nil {
		return nil, err
	}
	var deleted uint32
	if req.GetCascade() {
		// The claim keeps a sync from starting while resources go.
		op := store.Operation{Kind: "delete", Phase: gitops.PhaseRunning, InitiatedBy: p.UserID, StartedAt: time.Now().UTC()}
		claimed, err := s.svc.Store.ClaimOperation(ctx, p.WorkspaceID, a.ID, op, 5*time.Minute)
		if err != nil {
			return nil, s.storeErr(err, "application")
		}
		if !claimed {
			return nil, gitops.ErrOperationRunning
		}
		deleted, err = s.svc.DeleteResources(ctx, a)
		if err != nil {
			_ = s.svc.Store.FinishOperation(context.WithoutCancel(ctx), p.WorkspaceID, a.ID, a.Operation, nil)
			return nil, err
		}
	} else if a.OperationRunning(time.Now()) {
		return nil, gitops.ErrOperationRunning
	}
	if err := s.svc.Store.DeleteApplication(ctx, p.WorkspaceID, a.ID); err != nil {
		return nil, s.storeErr(err, "application")
	}
	s.svc.Audit.RecordAudit(p, "application.delete", "application", a.ID.String(),
		fmt.Sprintf("%s (cascade: %t, %d resources deleted)", a.Name, req.GetCascade(), deleted))
	s.svc.Events.Application(p.WorkspaceID, a.ID, a.ProjectID, "deleted", "", map[string]string{"name": a.Name})
	return &gitopsv1.DeleteApplicationResponse{DeletedResources: deleted}, nil
}

func (s *Server) RefreshApplication(ctx context.Context, req *gitopsv1.RefreshApplicationRequest) (*gitopsv1.Application, error) {
	p, err := caller(ctx, principal.Member)
	if err != nil {
		return nil, err
	}
	a, err := s.app(ctx, p, req.GetId())
	if err != nil {
		return nil, err
	}
	refreshed, err := s.svc.Refresh(ctx, a, req.GetHard())
	if err != nil {
		return nil, s.storeErr(err, "application")
	}
	s.svc.Audit.RecordAudit(p, "application.refresh", "application", a.ID.String(), fmt.Sprintf("hard: %t", req.GetHard()))
	return applicationToProto(refreshed, true), nil
}

func (s *Server) GetManifests(ctx context.Context, req *gitopsv1.GetManifestsRequest) (*gitopsv1.Manifests, error) {
	p, err := caller(ctx, principal.Viewer)
	if err != nil {
		return nil, err
	}
	a, err := s.app(ctx, p, req.GetId())
	if err != nil {
		return nil, err
	}
	rev, docs, err := s.svc.Manifests(ctx, a, strings.TrimSpace(req.GetRevision()))
	if err != nil {
		return nil, err
	}
	return &gitopsv1.Manifests{Revision: rev, Manifests: docs}, nil
}

func (s *Server) DiffApplication(ctx context.Context, req *gitopsv1.DiffApplicationRequest) (*gitopsv1.ApplicationDiff, error) {
	p, err := caller(ctx, principal.Viewer)
	if err != nil {
		return nil, err
	}
	a, err := s.app(ctx, p, req.GetId())
	if err != nil {
		return nil, err
	}
	rev, diffs, err := s.svc.Diff(ctx, a)
	if err != nil {
		return nil, err
	}
	out := &gitopsv1.ApplicationDiff{Revision: rev}
	for _, d := range diffs {
		out.Resources = append(out.Resources, &gitopsv1.ResourceDiff{
			Group: d.Group, Kind: d.Kind, Namespace: d.Namespace, Name: d.Name,
			SyncStatus: d.SyncStatus, Diff: d.Diff, Live: d.Live, Target: d.Target,
		})
	}
	return out, nil
}

// --- Operations ------------------------------------------------------------------------

func refs(in []*gitopsv1.ResourceRef) ([]gitops.ResourceRef, error) {
	if len(in) > 5000 {
		return nil, invalid("resources may name at most 5000 resources")
	}
	out := make([]gitops.ResourceRef, 0, len(in))
	for _, r := range in {
		if strings.TrimSpace(r.GetKind()) == "" || strings.TrimSpace(r.GetName()) == "" {
			return nil, invalid("every resource of a partial sync needs a kind and a name")
		}
		out = append(out, gitops.ResourceRef{Group: r.GetGroup(), Kind: r.GetKind(), Namespace: r.GetNamespace(), Name: r.GetName()})
	}
	return out, nil
}

func (s *Server) SyncApplication(ctx context.Context, req *gitopsv1.SyncApplicationRequest) (*gitopsv1.SyncResult, error) {
	p, err := caller(ctx, principal.Approver)
	if err != nil {
		return nil, err
	}
	a, err := s.app(ctx, p, req.GetId())
	if err != nil {
		return nil, err
	}
	partial, err := refs(req.GetResources())
	if err != nil {
		return nil, err
	}
	if err := cleanOverrides(req.GetKustomizeImages(), req.GetHelmParameters()); err != nil {
		return nil, err
	}
	revision := strings.TrimSpace(req.GetRevision())
	if !printable(revision, 255) {
		return nil, invalid("revision must be a branch, tag or commit SHA")
	}
	out, err := s.svc.Sync(ctx, a, gitops.SyncRequest{
		Kind:            gitops.KindSync,
		Revision:        revision,
		Prune:           req.GetPrune(),
		DryRun:          req.GetDryRun(),
		Resources:       partial,
		KustomizeImages: req.GetKustomizeImages(),
		HelmParameters:  req.GetHelmParameters(),
		ExecutionID:     strings.TrimSpace(req.GetExecutionId()),
		Timeout:         time.Duration(req.GetTimeoutSeconds()) * time.Second,
		Principal:       p,
	})
	if err != nil {
		return nil, s.storeErr(err, "application")
	}
	return outcomeToProto(out), nil
}

func (s *Server) RollbackApplication(ctx context.Context, req *gitopsv1.RollbackApplicationRequest) (*gitopsv1.SyncResult, error) {
	p, err := caller(ctx, principal.Approver)
	if err != nil {
		return nil, err
	}
	a, err := s.app(ctx, p, req.GetId())
	if err != nil {
		return nil, err
	}
	if a.Policy.Automated {
		// As in Argo CD: the next reconcile would sync forward again at once.
		return nil, status.Error(codes.FailedPrecondition, "turn off automated sync before rolling back; otherwise the next reconcile syncs the current source again")
	}
	historyID, err := parseID(req.GetHistoryId(), "history_id")
	if err != nil {
		return nil, err
	}
	entry, err := s.svc.Store.GetHistory(ctx, p.WorkspaceID, a.ID, historyID)
	if err != nil {
		return nil, s.storeErr(err, "history entry")
	}
	if entry.Revision == "" {
		return nil, status.Error(codes.FailedPrecondition, "that history entry rendered no revision; there is nothing to roll back to")
	}
	src := entry.Source
	out, err := s.svc.Sync(ctx, a, gitops.SyncRequest{
		Kind:        gitops.KindRollback,
		Revision:    entry.Revision,
		Source:      &src,
		Prune:       req.GetPrune(),
		ExecutionID: strings.TrimSpace(req.GetExecutionId()),
		Principal:   p,
	})
	if err != nil {
		return nil, s.storeErr(err, "application")
	}
	return outcomeToProto(out), nil
}

func (s *Server) ListHistory(ctx context.Context, req *gitopsv1.ListHistoryRequest) (*gitopsv1.ListHistoryResponse, error) {
	p, err := caller(ctx, principal.Viewer)
	if err != nil {
		return nil, err
	}
	a, err := s.app(ctx, p, req.GetId())
	if err != nil {
		return nil, err
	}
	limit := int(req.GetLimit())
	if limit <= 0 || limit > store.MaxHistory {
		limit = store.MaxHistory
	}
	rows, err := s.svc.Store.ListHistory(ctx, p.WorkspaceID, a.ID, limit)
	if err != nil {
		return nil, s.storeErr(err, "application")
	}
	out := &gitopsv1.ListHistoryResponse{}
	for _, h := range rows {
		out.Entries = append(out.Entries, historyToProto(h))
	}
	return out, nil
}

// --- Events and workspace data --------------------------------------------------------------

func (s *Server) WatchEvents(_ *commonv1.WatchEventsRequest, stream grpc.ServerStreamingServer[commonv1.PlatformEvent]) error {
	p, err := caller(stream.Context(), principal.Viewer)
	if err != nil {
		return err
	}
	sub := s.svc.Events.Subscribe(p.WorkspaceID)
	if sub == nil {
		return status.Error(codes.ResourceExhausted, "too many event streams are open")
	}
	defer sub.Close()
	for {
		select {
		case <-stream.Context().Done():
			return nil
		case <-s.Leaving.Done():
			// The feed never ends on its own; shutdown ends it so graceful
			// shutdown is not pinned by an open browser tab.
			return nil
		case e := <-sub.C:
			if err := stream.Send(e); err != nil {
				return nil
			}
		}
	}
}

// checkWorkspace rejects a body that names another workspace than the
// caller's: metadata is the authority, the body confirms the intent.
func checkWorkspace(p principal.Principal, raw string) error {
	named := strings.TrimSpace(raw)
	if named == "" {
		return invalid("workspace_id is required")
	}
	if named != p.WorkspaceID.String() {
		return status.Error(codes.PermissionDenied, "workspace_id does not match the caller's workspace")
	}
	return nil
}

func (s *Server) DeleteWorkspaceData(ctx context.Context, req *gitopsv1.DeleteWorkspaceDataRequest) (*gitopsv1.DeleteWorkspaceDataResponse, error) {
	p, err := caller(ctx, principal.Owner)
	if err != nil {
		return nil, err
	}
	if err := checkWorkspace(p, req.GetWorkspaceId()); err != nil {
		return nil, err
	}
	deleted, err := s.svc.Store.DeleteWorkspace(ctx, p.WorkspaceID)
	if err != nil {
		return nil, s.storeErr(err, "workspace")
	}
	s.svc.Clusters.DropWorkspace(p.WorkspaceID)
	s.svc.RenderCache.DeletePrefix(p.WorkspaceID.String() + "|")
	if s.ForgetWorkspace != nil {
		s.ForgetWorkspace(p.WorkspaceID)
	}
	s.log.Info("workspace data deleted", "workspace", p.WorkspaceID, "deleted", deleted)
	return &gitopsv1.DeleteWorkspaceDataResponse{Deleted: deleted}, nil
}

func (s *Server) ExportWorkspaceData(req *gitopsv1.ExportWorkspaceDataRequest, stream grpc.ServerStreamingServer[gitopsv1.WorkspaceDataRecord]) error {
	ctx := stream.Context()
	p, err := caller(ctx, principal.Admin)
	if err != nil {
		return err
	}
	if err := checkWorkspace(p, req.GetWorkspaceId()); err != nil {
		return err
	}
	err = s.svc.Store.ExportWorkspace(ctx, p.WorkspaceID, func(entity, json string) error {
		return stream.Send(&gitopsv1.WorkspaceDataRecord{Entity: entity, Json: json})
	})
	if err != nil {
		if _, ok := status.FromError(err); ok && status.Code(err) != codes.Unknown {
			return err
		}
		return s.storeErr(err, "workspace")
	}
	return nil
}

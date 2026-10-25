package server

import (
	"time"

	"google.golang.org/protobuf/types/known/timestamppb"

	gitopsv1 "github.com/f8fvwgzc/software-delivery-intelligence/services/gitops/gen/sdi/gitops/v1"
	"github.com/f8fvwgzc/software-delivery-intelligence/services/gitops/internal/gitops"
	"github.com/f8fvwgzc/software-delivery-intelligence/services/gitops/internal/store"
)

func ts(t *time.Time) *timestamppb.Timestamp {
	if t == nil || t.IsZero() {
		return nil
	}
	return timestamppb.New(*t)
}

func tsv(t time.Time) *timestamppb.Timestamp {
	if t.IsZero() {
		return nil
	}
	return timestamppb.New(t)
}

func clusterToProto(c store.Cluster) *gitopsv1.Cluster {
	return &gitopsv1.Cluster{
		Id:               c.ID.String(),
		Name:             c.Name,
		KubeconfigSecret: c.KubeconfigSecret,
		Context:          c.Context,
		Server:           c.Server,
		DefaultNamespace: c.DefaultNamespace,
		Namespaces:       c.Namespaces,
		Status:           c.Status,
		Message:          c.Message,
		ServerVersion:    c.ServerVersion,
		ApplicationCount: uint32(c.ApplicationCount),
		CheckedAt:        ts(c.CheckedAt),
		CreatedAt:        tsv(c.CreatedAt),
		UpdatedAt:        tsv(c.UpdatedAt),
	}
}

func sourceToProto(s store.Source) *gitopsv1.ApplicationSource {
	return &gitopsv1.ApplicationSource{
		Repo:            s.Repo,
		Revision:        s.Revision,
		Path:            s.Path,
		Type:            s.Type,
		Recurse:         s.Recurse,
		HelmValueFiles:  s.HelmValueFiles,
		HelmParameters:  s.HelmParameters,
		HelmReleaseName: s.HelmReleaseName,
		KustomizeImages: s.KustomizeImages,
	}
}

func sourceFromProto(p *gitopsv1.ApplicationSource) store.Source {
	if p == nil {
		return store.Source{}
	}
	return store.Source{
		Repo:            p.GetRepo(),
		Revision:        p.GetRevision(),
		Path:            p.GetPath(),
		Type:            p.GetType(),
		Recurse:         p.GetRecurse(),
		HelmValueFiles:  p.GetHelmValueFiles(),
		HelmParameters:  p.GetHelmParameters(),
		HelmReleaseName: p.GetHelmReleaseName(),
		KustomizeImages: p.GetKustomizeImages(),
	}
}

func policyToProto(p store.SyncPolicy) *gitopsv1.SyncPolicy {
	return &gitopsv1.SyncPolicy{
		Automated:       p.Automated,
		Prune:           p.Prune,
		SelfHeal:        p.SelfHeal,
		CreateNamespace: p.CreateNamespace,
		RetryLimit:      p.RetryLimit,
	}
}

func policyFromProto(p *gitopsv1.SyncPolicy) store.SyncPolicy {
	if p == nil {
		return store.SyncPolicy{}
	}
	return store.SyncPolicy{
		Automated:       p.GetAutomated(),
		Prune:           p.GetPrune(),
		SelfHeal:        p.GetSelfHeal(),
		CreateNamespace: p.GetCreateNamespace(),
		RetryLimit:      p.GetRetryLimit(),
	}
}

func operationToProto(a store.Application, now time.Time) *gitopsv1.Operation {
	op := a.Operation
	if op == nil {
		return nil
	}
	out := &gitopsv1.Operation{
		Kind:        op.Kind,
		Phase:       op.Phase,
		Message:     op.Message,
		Revision:    op.Revision,
		InitiatedBy: op.InitiatedBy,
		ExecutionId: op.ExecutionID,
		StartedAt:   tsv(op.StartedAt),
		FinishedAt:  ts(op.FinishedAt),
	}
	if op.Phase == gitops.PhaseRunning && !a.OperationRunning(now) {
		// The replica running it stopped before it could record the end.
		out.Phase = gitops.PhaseError
		out.Message = "the operation was interrupted before it finished (the service stopped)"
	}
	return out
}

// applicationToProto converts an application; full adds the resource tree
// and every condition (GetApplication), summaries keep the first condition.
func applicationToProto(a store.Application, full bool) *gitopsv1.Application {
	out := &gitopsv1.Application{
		Id:             a.ID.String(),
		ProjectId:      a.ProjectID,
		Name:           a.Name,
		ClusterId:      a.ClusterID.String(),
		ClusterName:    a.ClusterName,
		Namespace:      a.Namespace,
		Source:         sourceToProto(a.Source),
		SyncPolicy:     policyToProto(a.Policy),
		SyncStatus:     a.SyncStatus,
		HealthStatus:   a.HealthStatus,
		HealthMessage:  a.HealthMessage,
		Revision:       a.Revision,
		SyncedRevision: a.SyncedRevision,
		Operation:      operationToProto(a, time.Now()),
		ResourceCount:  uint32(a.ResourceCount),
		OutOfSyncCount: uint32(a.OutOfSyncCount),
		ReconciledAt:   ts(a.ReconciledAt),
		CreatedAt:      tsv(a.CreatedAt),
		UpdatedAt:      tsv(a.UpdatedAt),
	}
	if full {
		out.Conditions = a.Conditions
		for _, r := range a.Resources {
			out.Resources = append(out.Resources, &gitopsv1.ResourceStatus{
				Group: r.Group, Version: r.Version, Kind: r.Kind, Namespace: r.Namespace, Name: r.Name,
				SyncStatus: r.SyncStatus, HealthStatus: r.HealthStatus, HealthMessage: r.HealthMessage,
				Hook: r.Hook, SyncWave: r.SyncWave,
			})
		}
	} else if len(a.Conditions) > 0 {
		out.Conditions = a.Conditions[:1]
	}
	return out
}

func historyToProto(h store.HistoryEntry) *gitopsv1.HistoryEntry {
	return &gitopsv1.HistoryEntry{
		Id:            h.ID.String(),
		ApplicationId: h.ApplicationID.String(),
		Revision:      h.Revision,
		Source:        sourceToProto(h.Source),
		Phase:         h.Phase,
		Message:       h.Message,
		InitiatedBy:   h.InitiatedBy,
		ExecutionId:   h.ExecutionID,
		Automated:     h.Automated,
		StartedAt:     tsv(h.StartedAt),
		FinishedAt:    tsv(h.FinishedAt),
		Kind:          h.Kind,
	}
}

func outcomeToProto(o *gitops.SyncOutcome) *gitopsv1.SyncResult {
	out := &gitopsv1.SyncResult{
		ApplicationId: o.ApplicationID.String(),
		Phase:         o.Phase,
		Message:       o.Message,
		Revision:      o.Revision,
		HistoryId:     o.HistoryID,
		HealthStatus:  o.HealthStatus,
		StartedAt:     tsv(o.StartedAt),
		FinishedAt:    tsv(o.FinishedAt),
	}
	for _, r := range o.Resources {
		out.Resources = append(out.Resources, &gitopsv1.ResourceResult{
			Group: r.Group, Kind: r.Kind, Namespace: r.Namespace, Name: r.Name,
			Status: r.Status, Message: r.Message, HookPhase: r.HookPhase, SyncWave: r.SyncWave,
		})
	}
	return out
}

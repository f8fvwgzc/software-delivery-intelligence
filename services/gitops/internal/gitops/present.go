package gitops

import (
	"context"
	"encoding/json"
	"fmt"
	"sort"
	"strings"
	"time"

	"github.com/pmezard/go-difflib/difflib"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/status"
	metav1 "k8s.io/apimachinery/pkg/apis/meta/v1"
	"k8s.io/apimachinery/pkg/apis/meta/v1/unstructured"
	"sigs.k8s.io/yaml"

	enginecache "github.com/argoproj/argo-cd/gitops-engine/pkg/cache"
	hookutil "github.com/argoproj/argo-cd/gitops-engine/pkg/sync/hook"
	"github.com/argoproj/argo-cd/gitops-engine/pkg/utils/kube"

	"github.com/f8fvwgzc/software-delivery-intelligence/services/gitops/internal/clusters"
	"github.com/f8fvwgzc/software-delivery-intelligence/services/gitops/internal/mask"
	"github.com/f8fvwgzc/software-delivery-intelligence/services/gitops/internal/store"
	"github.com/f8fvwgzc/software-delivery-intelligence/services/gitops/internal/tracking"
)

const (
	// MaxResponseBytes bounds a manifests or diff answer, well inside the
	// 16 MiB gRPC message limit.
	MaxResponseBytes = 12 << 20
	// maxTextBytes bounds one resource's live, target or diff text.
	maxTextBytes = 256 << 10
	// clusterProbe bounds how long a read waits for the cluster before it
	// renders without the cluster's capabilities.
	clusterProbe = 15 * time.Second
)

// kindOrder is the dependency order of kinds within a wave, the engine's
// (and Helm's) install order.
var kindOrder = func() map[string]int {
	kinds := []string{"Namespace", "NetworkPolicy", "ResourceQuota", "LimitRange", "PodSecurityPolicy",
		"PodDisruptionBudget", "ServiceAccount", "Secret", "SecretList", "ConfigMap", "StorageClass",
		"PersistentVolume", "PersistentVolumeClaim", "CustomResourceDefinition", "ClusterRole",
		"ClusterRoleList", "ClusterRoleBinding", "ClusterRoleBindingList", "Role", "RoleList", "RoleBinding",
		"RoleBindingList", "Service", "DaemonSet", "Pod", "ReplicationController", "ReplicaSet", "Deployment",
		"HorizontalPodAutoscaler", "StatefulSet", "Job", "CronJob", "IngressClass", "Ingress", "APIService"}
	m := map[string]int{}
	for i, k := range kinds {
		m[k] = i - len(kinds)
	}
	return m
}()

// SortManifests orders resources by sync wave, then kind order, then name.
func SortManifests(objs []*unstructured.Unstructured) {
	sort.SliceStable(objs, func(i, j int) bool {
		a, b := objs[i], objs[j]
		if wa, wb := wave(a), wave(b); wa != wb {
			return wa < wb
		}
		if ka, kb := kindOrder[a.GetKind()], kindOrder[b.GetKind()]; ka != kb {
			return ka < kb
		}
		if a.GetNamespace() != b.GetNamespace() {
			return a.GetNamespace() < b.GetNamespace()
		}
		return a.GetName() < b.GetName()
	})
}

// probeCluster acquires the application's cluster when it answers soon;
// nil otherwise (a read can still render without it).
func (s *Service) probeCluster(ctx context.Context, app store.Application) *clusters.Handle {
	cluster, err := s.Store.GetCluster(ctx, app.WorkspaceID, app.ClusterID)
	if err != nil {
		return nil
	}
	pctx, cancel := context.WithTimeout(ctx, clusterProbe)
	defer cancel()
	h, err := s.Clusters.Acquire(pctx, cluster)
	if err != nil {
		return nil
	}
	return h
}

// Manifests is the desired state as rendered, one masked YAML document per
// resource (CONTRACT.md §24.4).
func (s *Service) Manifests(ctx context.Context, app store.Application, revision string) (string, []string, error) {
	h := s.probeCluster(ctx, app)
	if h != nil {
		defer h.Release()
	}
	d, err := s.Desired(ctx, app, app.Source, revision, h, false)
	if err != nil {
		return "", nil, status.Error(codes.FailedPrecondition, sentence(err.Error()))
	}
	SortManifests(d.Objects)
	docs := make([]string, 0, len(d.Objects))
	total := 0
	for _, obj := range d.Objects {
		text, err := maskedYAML(obj)
		if err != nil {
			return "", nil, status.Error(codes.Internal, "a manifest could not be serialised")
		}
		total += len(text)
		if total > MaxResponseBytes {
			return "", nil, status.Errorf(codes.ResourceExhausted, "the rendered manifests exceed %d MiB", MaxResponseBytes>>20)
		}
		docs = append(docs, text)
	}
	return d.Revision, docs, nil
}

func maskedYAML(obj *unstructured.Unstructured) (string, error) {
	masked, err := mask.One(obj)
	if err != nil {
		masked = mask.Redacted(obj)
	}
	raw, err := yaml.Marshal(masked.Object)
	return string(raw), err
}

// ResourceDiff is one resource's difference between live and desired.
type ResourceDiff struct {
	Group, Kind, Namespace, Name string
	SyncStatus                   string
	Diff, Live, Target           string
}

// Diff compares now and answers each resource's normalised live and
// desired YAML (Secrets masked, live and desired alike) and the unified
// diff between them.
func (s *Service) Diff(ctx context.Context, app store.Application) (string, []ResourceDiff, error) {
	var h *clusters.Handle
	cmp, conditions := s.CompareNow(ctx, app, false, &h)
	if cmp == nil {
		return "", nil, status.Error(codes.FailedPrecondition, strings.Join(conditions, " "))
	}
	defer h.Release()
	rec := cmp.Reconciliation
	var out []ResourceDiff
	total := 0
	for i := range rec.Target {
		target, live := rec.Target[i], rec.Live[i]
		obj := live
		if obj == nil {
			obj = target
		}
		if obj == nil || (target == nil && hookutil.IsHook(live)) {
			continue
		}
		key := kube.GetResourceKey(obj)
		modified := i < len(cmp.Diffs) && cmp.Diffs[i].Modified
		rd := ResourceDiff{Group: key.Group, Kind: key.Kind, Namespace: key.Namespace, Name: key.Name,
			SyncStatus: resourceSyncStatus(target, live, modified, cmp.Foreign[key] != "", false)}
		var normalizedLive, predicted *unstructured.Unstructured
		if i < len(cmp.Diffs) {
			normalizedLive = fromJSON(cmp.Diffs[i].NormalizedLive)
			predicted = fromJSON(cmp.Diffs[i].PredictedLive)
		}
		if live == nil {
			normalizedLive = nil
		}
		if target == nil {
			predicted = nil
		}
		maskedTarget, maskedLive, err := mask.Pair(predicted, normalizedLive)
		if err != nil {
			maskedTarget, maskedLive = mask.Redacted(predicted), mask.Redacted(normalizedLive)
		}
		rd.Live, rd.Target = displayYAML(maskedLive), displayYAML(maskedTarget)
		if rd.Live != rd.Target {
			rd.Diff = unified(rd.Live, rd.Target)
		}
		rd.Live, rd.Target, rd.Diff = clip(rd.Live), clip(rd.Target), clip(rd.Diff)
		size := len(rd.Live) + len(rd.Target) + len(rd.Diff)
		if total+size > MaxResponseBytes {
			// Past the budget only the differences are answered.
			rd.Live, rd.Target = "", ""
			size = len(rd.Diff)
			if total+size > MaxResponseBytes {
				rd.Diff = "(omitted: the answer reached its size limit)"
				size = len(rd.Diff)
			}
		}
		total += size
		out = append(out, rd)
	}
	sort.SliceStable(out, func(i, j int) bool {
		a, b := out[i], out[j]
		if a.Kind != b.Kind {
			return a.Kind < b.Kind
		}
		if a.Namespace != b.Namespace {
			return a.Namespace < b.Namespace
		}
		return a.Name < b.Name
	})
	return cmp.Desired.Revision, out, nil
}

func fromJSON(raw []byte) *unstructured.Unstructured {
	if len(raw) == 0 || string(raw) == "null" {
		return nil
	}
	var obj map[string]any
	if err := json.Unmarshal(raw, &obj); err != nil || obj == nil {
		return nil
	}
	return &unstructured.Unstructured{Object: obj}
}

// displayYAML is an object for reading: without managed fields, which are
// bookkeeping of the API server and longer than the object itself.
func displayYAML(obj *unstructured.Unstructured) string {
	if obj == nil {
		return ""
	}
	obj = obj.DeepCopy()
	unstructured.RemoveNestedField(obj.Object, "metadata", "managedFields")
	raw, err := yaml.Marshal(obj.Object)
	if err != nil {
		return ""
	}
	return string(raw)
}

func unified(live, target string) string {
	text, err := difflib.GetUnifiedDiffString(difflib.UnifiedDiff{
		A: difflib.SplitLines(live), B: difflib.SplitLines(target),
		FromFile: "live", ToFile: "desired", Context: 3,
	})
	if err != nil {
		return ""
	}
	return text
}

func clip(s string) string {
	if len(s) <= maxTextBytes {
		return s
	}
	return s[:maxTextBytes] + "\n... (truncated)\n"
}

// DeleteResources deletes from the cluster every resource that carries
// this application's own tracking id (a cascading delete) and answers how
// many were deleted. Nothing else is touched.
func (s *Service) DeleteResources(ctx context.Context, app store.Application) (uint32, error) {
	cluster, err := s.Store.GetCluster(ctx, app.WorkspaceID, app.ClusterID)
	if err != nil {
		return 0, err
	}
	h, err := s.Clusters.Acquire(ctx, cluster)
	if err != nil {
		return 0, status.Error(codes.Unavailable, err.Error())
	}
	defer h.Release()
	appID := app.ID.String()
	owned := h.Cache().FindResources("", func(r *enginecache.Resource) bool {
		return clusters.AppOf(r) == appID && r.Resource != nil && tracking.Owns(appID, r.Resource)
	})
	foreground := metav1.DeletePropagationForeground
	var deleted uint32
	var failures []string
	for _, r := range owned {
		gvk := r.Ref.GroupVersionKind()
		err := s.Clusters.Kubectl().DeleteResource(ctx, h.Config(), gvk, r.Ref.Name, r.Ref.Namespace,
			metav1.DeleteOptions{PropagationPolicy: &foreground})
		if err != nil && !strings.Contains(err.Error(), "not found") {
			failures = append(failures, fmt.Sprintf("%s %s: %s", r.Ref.Kind, r.Ref.Name, oneLine(err.Error())))
			continue
		}
		deleted++
	}
	if len(failures) > 0 {
		return deleted, status.Errorf(codes.Internal, "%d resource(s) could not be deleted: %s", len(failures), strings.Join(failures, "; "))
	}
	return deleted, nil
}

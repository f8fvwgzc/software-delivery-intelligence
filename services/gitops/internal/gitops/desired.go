package gitops

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"slices"
	"sort"
	"strings"

	"github.com/google/uuid"
	"google.golang.org/grpc/status"
	"k8s.io/apimachinery/pkg/apis/meta/v1/unstructured"

	"github.com/argoproj/argo-cd/gitops-engine/pkg/sync/syncwaves"
	"github.com/argoproj/argo-cd/gitops-engine/pkg/utils/kube"

	"github.com/f8fvwgzc/software-delivery-intelligence/services/gitops/internal/clusters"
	"github.com/f8fvwgzc/software-delivery-intelligence/services/gitops/internal/github"
	"github.com/f8fvwgzc/software-delivery-intelligence/services/gitops/internal/principal"
	"github.com/f8fvwgzc/software-delivery-intelligence/services/gitops/internal/render"
	"github.com/f8fvwgzc/software-delivery-intelligence/services/gitops/internal/store"
	"github.com/f8fvwgzc/software-delivery-intelligence/services/gitops/internal/tracking"
)

// Desired is an application's rendered desired state at one commit.
type Desired struct {
	Repo     string
	Revision string
	Source   store.Source
	Type     string
	// Objects are the rendered resources (hooks included), each in its
	// final namespace and carrying the tracking annotation.
	Objects []*unstructured.Unstructured
	// Digest identifies the rendered state: automation tells a new desired
	// state from drift by it.
	Digest string
	// Warnings are sentences about the render that did not stop it
	// (duplicate resources).
	Warnings []string
}

// SourceError is a failure to obtain or render the desired state: a
// condition of the application, never an RPC failure.
type SourceError struct {
	Message string
}

func (e *SourceError) Error() string { return e.Message }

func sourceErrorf(format string, args ...any) error {
	return &SourceError{Message: fmt.Sprintf(format, args...)}
}

// repoAndRef is the repository and the revision to read: the source's, or
// the project's repository and default branch.
func (s *Service) repoAndRef(ctx context.Context, app store.Application, src store.Source, revision string) (string, string, error) {
	repo := strings.TrimSpace(src.Repo)
	ref := strings.TrimSpace(revision)
	if ref == "" {
		ref = strings.TrimSpace(src.Revision)
	}
	if repo == "" {
		project, err := s.Projects.GetProject(ctx, principal.System(app.WorkspaceID), app.ProjectID)
		if err != nil {
			return "", "", sourceErrorf("the project's repository could not be read: %s", status.Convert(err).Message())
		}
		repo = project.RepoFullName
		if repo == "" {
			return "", "", sourceErrorf("the project has no repository")
		}
		if ref == "" {
			ref = project.DefaultBranch
		}
	}
	if ref == "" {
		branch, err := s.Repos.DefaultBranch(ctx, app.WorkspaceID, repo)
		if err != nil {
			return "", "", sourceErrorf("the source could not be read: %s", err.Error())
		}
		ref = branch
	}
	return repo, ref, nil
}

// clusterInfo is what the render needs to know of the target cluster.
type clusterInfo struct {
	kubeVersion string
	apiVersions []string
	scope       kube.ResourceInfoProvider
}

func infoOf(h *clusters.Handle) clusterInfo {
	if h == nil {
		return clusterInfo{}
	}
	return clusterInfo{kubeVersion: h.ServerVersion(), apiVersions: h.APIVersions(), scope: h.Cache()}
}

// renderKey identifies one render: everything that changes its output.
func renderKey(workspace uuid.UUID, repo, sha, namespace string, src store.Source, info clusterInfo) string {
	h := sha256.New()
	apis := slices.Clone(info.apiVersions)
	slices.Sort(apis)
	payload, _ := json.Marshal(struct {
		Repo, SHA, Namespace string
		Source               store.Source
		KubeVersion          string
		APIs                 []string
	}{repo, sha, namespace, src, info.kubeVersion, apis})
	h.Write(payload)
	return workspace.String() + "|" + hex.EncodeToString(h.Sum(nil))
}

// Desired renders src (the application's source unless overridden) at
// revision (the source's unless set) for the application's destination.
// hard skips and replaces the cached render.
func (s *Service) Desired(ctx context.Context, app store.Application, src store.Source, revision string, h *clusters.Handle, hard bool) (*Desired, error) {
	repo, ref, err := s.repoAndRef(ctx, app, src, revision)
	if err != nil {
		return nil, err
	}
	sha, err := s.Repos.ResolveCommit(ctx, app.WorkspaceID, repo, ref)
	if err != nil {
		return nil, sourceErrorf("the source could not be read: %s", err.Error())
	}
	info := infoOf(h)
	key := renderKey(app.WorkspaceID, repo, sha, app.Namespace, src, info)
	if hard {
		s.RenderCache.Delete(key)
	}
	objects, kind, ok := s.RenderCache.Get(key)
	if !ok {
		objects, kind, err = s.renderAt(ctx, app, src, repo, sha, info)
		if err != nil {
			return nil, err
		}
		s.RenderCache.Put(key, kind, objects, sizeOf(objects))
	}
	d := &Desired{Repo: repo, Revision: sha, Source: src, Type: kind}
	d.Objects, d.Warnings = s.normalize(app, objects, info.scope)
	d.Digest = digest(d.Objects)
	return d, nil
}

func (s *Service) renderAt(ctx context.Context, app store.Application, src store.Source, repo, sha string, info clusterInfo) ([]*unstructured.Unstructured, string, error) {
	select {
	case s.renderSlots <- struct{}{}:
	case <-ctx.Done():
		return nil, "", ctx.Err()
	}
	defer func() { <-s.renderSlots }()
	checkout, err := s.Repos.Download(ctx, app.WorkspaceID, repo, sha)
	if err != nil {
		var gerr *github.Error
		if errors.As(err, &gerr) {
			return nil, "", sourceErrorf("the source could not be read: %s", gerr.Message)
		}
		return nil, "", sourceErrorf("the source could not be checked out")
	}
	defer checkout.Remove()
	release := src.HelmReleaseName
	if release == "" {
		release = app.Name
	}
	res, err := render.Render(ctx, render.Request{
		Root:            checkout.Dir,
		Path:            src.Path,
		Type:            src.Type,
		Recurse:         src.Recurse,
		HelmValueFiles:  src.HelmValueFiles,
		HelmParameters:  src.HelmParameters,
		HelmReleaseName: release,
		KustomizeImages: src.KustomizeImages,
		Namespace:       app.Namespace,
		KubeVersion:     info.kubeVersion,
		APIVersions:     info.apiVersions,
		Oversized:       checkout.Oversized,
	}, s.Limits)
	if err != nil {
		if rerr, ok := render.AsError(err); ok {
			return nil, "", sourceErrorf("rendering failed: %s", rerr.Message)
		}
		return nil, "", sourceErrorf("rendering failed: %s", err.Error())
	}
	return res.Objects, res.Type, nil
}

// normalize puts every resource in its final namespace, keeps the last of
// duplicate definitions (as Argo CD does, with a warning) and writes the
// tracking annotation.
func (s *Service) normalize(app store.Application, objects []*unstructured.Unstructured, scope kube.ResourceInfoProvider) ([]*unstructured.Unstructured, []string) {
	appID := app.ID.String()
	var warnings []string
	index := map[kube.ResourceKey]int{}
	out := make([]*unstructured.Unstructured, 0, len(objects))
	for _, obj := range objects {
		if obj.GetNamespace() == "" {
			namespaced := true
			if scope != nil {
				namespaced = kube.IsNamespacedOrUnknown(scope, obj.GroupVersionKind().GroupKind())
			} else {
				namespaced = !knownClusterScoped[obj.GroupVersionKind().GroupKind().String()]
			}
			if namespaced {
				obj.SetNamespace(app.Namespace)
			}
		}
		tracking.Set(appID, obj)
		key := kube.GetResourceKey(obj)
		if obj.GetName() == "" {
			// generateName hooks are unique per sync, never duplicates.
			out = append(out, obj)
			continue
		}
		if i, ok := index[key]; ok {
			warnings = append(warnings, fmt.Sprintf("%s %s is defined more than once; the last definition is used", obj.GetKind(), describeKey(key)))
			out[i] = obj
			continue
		}
		index[key] = len(out)
		out = append(out, obj)
	}
	return out, warnings
}

// knownClusterScoped are the built-in cluster-scoped kinds, for renders
// made without a cluster to ask.
var knownClusterScoped = map[string]bool{
	"Namespace": true, "Node": true, "PersistentVolume": true, "ClusterRole.rbac.authorization.k8s.io": true,
	"ClusterRoleBinding.rbac.authorization.k8s.io": true, "CustomResourceDefinition.apiextensions.k8s.io": true,
	"StorageClass.storage.k8s.io": true, "PriorityClass.scheduling.k8s.io": true, "IngressClass.networking.k8s.io": true,
	"MutatingWebhookConfiguration.admissionregistration.k8s.io":   true,
	"ValidatingWebhookConfiguration.admissionregistration.k8s.io": true,
	"APIService.apiregistration.k8s.io":                           true, "CSIDriver.storage.k8s.io": true,
	"RuntimeClass.node.k8s.io": true,
}

func describeKey(key kube.ResourceKey) string {
	if key.Namespace == "" {
		return key.Name
	}
	return key.Namespace + "/" + key.Name
}

// digest is a hash of the rendered state, independent of order.
func digest(objs []*unstructured.Unstructured) string {
	lines := make([]string, 0, len(objs))
	for _, o := range objs {
		raw, _ := json.Marshal(o.Object)
		lines = append(lines, string(raw))
	}
	sort.Strings(lines)
	h := sha256.New()
	for _, l := range lines {
		h.Write([]byte(l))
		h.Write([]byte{'\n'})
	}
	return hex.EncodeToString(h.Sum(nil))
}

func sizeOf(objs []*unstructured.Unstructured) int {
	n := 0
	for _, o := range objs {
		raw, _ := json.Marshal(o.Object)
		n += len(raw)
	}
	return n
}

// wave is a resource's sync wave.
func wave(obj *unstructured.Unstructured) int32 {
	if obj == nil {
		return 0
	}
	return int32(syncwaves.Wave(obj))
}

// Package render turns a checked-out source into the desired state: what
// Argo CD's repo-server does, in process and with no network (CONTRACT.md
// §24.3). Three source types: a directory of manifests, a kustomization
// (the kustomize Go API, confined to the checkout, no remote bases) and a
// Helm chart (the Helm Go SDK, client-only, dependencies vendored).
//
// Every error is an *Error whose message is a sentence for the person who
// owns the repository; it becomes a condition of the application, never a
// crash.
package render

import (
	"context"
	"errors"
	"fmt"
	"net/http"
	"os"
	"path"
	"path/filepath"
	"slices"
	"strings"
	"time"

	"k8s.io/apimachinery/pkg/apis/meta/v1/unstructured"
)

const (
	TypeDirectory = "directory"
	TypeKustomize = "kustomize"
	TypeHelm      = "helm"
)

// Limits bound what one source may render (§24.3).
type Limits struct {
	// MaxResources is the most resources a source may render.
	MaxResources int
	// MaxFileBytes is the largest file the checkout keeps.
	MaxFileBytes int64
	// Timeout bounds one render.
	Timeout time.Duration
}

// DefaultLimits are the contract's: 5 000 resources, 10 MB per file.
var DefaultLimits = Limits{MaxResources: 5000, MaxFileBytes: 10 << 20, Timeout: 2 * time.Minute}

// Error is a render failure, as a sentence.
type Error struct {
	Message string
}

func (e *Error) Error() string { return e.Message }

func errorf(format string, args ...any) error {
	return &Error{Message: fmt.Sprintf(format, args...)}
}

// AsError is the render failure inside err, if any.
func AsError(err error) (*Error, bool) {
	var e *Error
	ok := errors.As(err, &e)
	return e, ok
}

// Request is one render.
type Request struct {
	// Root is the checkout: the repository at one commit.
	Root string
	// Path is the directory inside the repository; "" is its root.
	Path string
	// Type is "directory", "kustomize", "helm" or "" (detect).
	Type            string
	Recurse         bool
	HelmValueFiles  []string
	HelmParameters  map[string]string
	HelmReleaseName string
	KustomizeImages []string
	// Namespace is the destination namespace (helm's release namespace).
	Namespace string
	// KubeVersion and APIVersions describe the target cluster to Helm
	// (.Capabilities), when known.
	KubeVersion string
	APIVersions []string
	// Oversized lists repository paths the checkout left out for exceeding
	// the file limit; one under Path fails the render.
	Oversized []string
}

// Result is the rendered desired state.
type Result struct {
	// Type is the source type that rendered (the detected one for "").
	Type    string
	Objects []*unstructured.Unstructured
}

// CleanPath validates a repository-relative directory: no absolute path,
// nothing that leaves the repository. "" and "." are the root.
func CleanPath(p string) (string, error) {
	p = strings.TrimSpace(p)
	if p == "" || p == "." || p == "/" {
		return "", nil
	}
	if strings.Contains(p, "\\") || strings.Contains(p, "\x00") {
		return "", fmt.Errorf("path %q is not a valid repository path", p)
	}
	if strings.HasPrefix(p, "/") {
		return "", fmt.Errorf("path %q must be relative to the repository root", p)
	}
	cleaned := path.Clean(p)
	if cleaned == ".." || strings.HasPrefix(cleaned, "../") {
		return "", fmt.Errorf("path %q leaves the repository", p)
	}
	return cleaned, nil
}

// Detect is the source type of a directory: a kustomization makes it
// kustomize, a Chart.yaml helm, anything else a directory of manifests.
func Detect(dir string) string {
	for _, name := range kustomizationNames {
		if isFile(filepath.Join(dir, name)) {
			return TypeKustomize
		}
	}
	if isFile(filepath.Join(dir, "Chart.yaml")) {
		return TypeHelm
	}
	return TypeDirectory
}

var kustomizationNames = []string{"kustomization.yaml", "kustomization.yml", "Kustomization"}

func isFile(p string) bool {
	info, err := os.Stat(p)
	return err == nil && info.Mode().IsRegular()
}

// Render renders req within limits. A panic inside a renderer (a template
// engine meeting a hostile chart) is a render error, and a render that
// outlives the timeout is abandoned.
func Render(ctx context.Context, req Request, limits Limits) (Result, error) {
	if limits.Timeout <= 0 {
		limits.Timeout = DefaultLimits.Timeout
	}
	type outcome struct {
		res Result
		err error
	}
	done := make(chan outcome, 1)
	go func() {
		defer func() {
			if r := recover(); r != nil {
				done <- outcome{err: errorf("rendering failed unexpectedly: %v", r)}
			}
		}()
		res, err := render(req, limits)
		done <- outcome{res, err}
	}()
	timer := time.NewTimer(limits.Timeout)
	defer timer.Stop()
	select {
	case o := <-done:
		return o.res, o.err
	case <-timer.C:
		return Result{}, errorf("rendering took longer than %s", limits.Timeout)
	case <-ctx.Done():
		return Result{}, ctx.Err()
	}
}

func render(req Request, limits Limits) (Result, error) {
	rel, err := CleanPath(req.Path)
	if err != nil {
		return Result{}, &Error{Message: err.Error()}
	}
	root, err := filepath.EvalSymlinks(req.Root)
	if err != nil {
		return Result{}, errorf("the checkout is not readable")
	}
	dir := filepath.Join(root, filepath.FromSlash(rel))
	shown := rel
	if shown == "" {
		shown = "the repository root"
	}
	info, err := os.Stat(dir)
	if err != nil || !info.IsDir() {
		return Result{}, errorf("%s is not a directory in the repository", shown)
	}
	for _, big := range req.Oversized {
		if rel == "" || big == rel || strings.HasPrefix(big, rel+"/") {
			return Result{}, errorf("%s is larger than %d MB, the limit for one file", big, limits.MaxFileBytes>>20)
		}
	}
	kind := req.Type
	if kind == "" {
		kind = Detect(dir)
	}
	var objs []*unstructured.Unstructured
	switch kind {
	case TypeDirectory:
		objs, err = renderDirectory(root, dir, req.Recurse, limits)
	case TypeKustomize:
		objs, err = renderKustomize(root, dir, req.KustomizeImages)
	case TypeHelm:
		objs, err = renderHelm(root, dir, req)
	default:
		return Result{}, errorf("source type %q is not one of directory, kustomize, helm", kind)
	}
	if err != nil {
		if _, ok := AsError(err); ok {
			return Result{}, err
		}
		return Result{}, &Error{Message: err.Error()}
	}
	objs, err = flatten(objs)
	if err != nil {
		return Result{}, err
	}
	if len(objs) > limits.MaxResources {
		return Result{}, errorf("the source renders %d resources; the limit is %d", len(objs), limits.MaxResources)
	}
	return Result{Type: kind, Objects: objs}, nil
}

// flatten expands `kind: List` documents into their items and drops empty
// documents, as kubectl does.
func flatten(objs []*unstructured.Unstructured) ([]*unstructured.Unstructured, error) {
	out := make([]*unstructured.Unstructured, 0, len(objs))
	for _, obj := range objs {
		if obj == nil || len(obj.Object) == 0 {
			continue
		}
		if obj.IsList() {
			list, err := obj.ToList()
			if err != nil {
				return nil, errorf("a %s could not be read as a list of resources", obj.GetKind())
			}
			for i := range list.Items {
				item := list.Items[i]
				out = append(out, &item)
			}
			continue
		}
		if obj.GetKind() == "" || obj.GetAPIVersion() == "" {
			return nil, errorf("a rendered document has no apiVersion or kind")
		}
		if obj.GetName() == "" && obj.GetGenerateName() == "" {
			return nil, errorf("a rendered %s has no metadata.name", obj.GetKind())
		}
		out = append(out, obj)
	}
	return out, nil
}

// insideRoot resolves p (relative to base) and checks it stays in root.
func insideRoot(root, base, p string) (string, error) {
	if strings.Contains(p, "://") {
		return "", fmt.Errorf("%s is a URL; only files in the repository can be used", p)
	}
	if filepath.IsAbs(p) {
		return "", fmt.Errorf("%s must be relative", p)
	}
	full := filepath.Clean(filepath.Join(base, filepath.FromSlash(p)))
	if full != root && !strings.HasPrefix(full, root+string(filepath.Separator)) {
		return "", fmt.Errorf("%s leaves the repository", p)
	}
	return full, nil
}

// sortedKeys is a map's keys in order, so renders are deterministic.
func sortedKeys(m map[string]string) []string {
	keys := make([]string, 0, len(m))
	for k := range m {
		keys = append(keys, k)
	}
	slices.Sort(keys)
	return keys
}

// BlockDefaultNetwork closes http.DefaultTransport for the whole process.
// Rendering must not reach the network (§24.3); kustomize's loader fetches
// remote files with a default client, so besides refusing every remote
// reference before kustomize sees it, the default transport is shut. The
// service's own HTTP clients (GitHub) carry transports of their own, and
// the Kubernetes clients build theirs from rest.Config.
func BlockDefaultNetwork() {
	http.DefaultTransport = refusingTransport{}
}

type refusingTransport struct{}

func (refusingTransport) RoundTrip(req *http.Request) (*http.Response, error) {
	return nil, fmt.Errorf("network access is not allowed while rendering (%s)", req.URL.Host)
}

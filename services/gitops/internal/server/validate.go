package server

import (
	"regexp"
	"slices"
	"strings"
	"unicode"

	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/status"

	"github.com/f8fvwgzc/software-delivery-intelligence/services/gitops/internal/github"
	"github.com/f8fvwgzc/software-delivery-intelligence/services/gitops/internal/render"
	"github.com/f8fvwgzc/software-delivery-intelligence/services/gitops/internal/store"
)

var (
	dnsLabel = regexp.MustCompile(`^[a-z0-9]([-a-z0-9]*[a-z0-9])?$`)
	// secretName is identity's rule for workspace secret names (§22).
	secretName = regexp.MustCompile(`^[A-Z][A-Z0-9_]{1,63}$`)
)

const (
	maxNamespaces     = 100
	maxValueFiles     = 20
	maxHelmParameters = 100
	maxImages         = 50
	maxRetryLimit     = 10
)

func invalid(format string, args ...any) error {
	return status.Errorf(codes.InvalidArgument, format, args...)
}

func validLabel(s string, max int) bool {
	return len(s) <= max && dnsLabel.MatchString(s)
}

func printable(s string, max int) bool {
	if len(s) > max {
		return false
	}
	for _, r := range s {
		if unicode.IsControl(r) {
			return false
		}
	}
	return true
}

// cleanCluster validates and normalises the fields UpsertCluster reads.
func cleanCluster(c store.Cluster) (store.Cluster, error) {
	c.Name = strings.TrimSpace(c.Name)
	if !validLabel(c.Name, 63) {
		return c, invalid("name must be lowercase letters, digits and '-', at most 63 characters")
	}
	c.KubeconfigSecret = strings.TrimSpace(c.KubeconfigSecret)
	if !secretName.MatchString(c.KubeconfigSecret) {
		return c, invalid("kubeconfig_secret must name a workspace secret: upper snake case, e.g. STAGING_KUBECONFIG")
	}
	c.Context = strings.TrimSpace(c.Context)
	if !printable(c.Context, 253) {
		return c, invalid("context must be at most 253 printable characters")
	}
	c.DefaultNamespace = strings.TrimSpace(c.DefaultNamespace)
	if c.DefaultNamespace == "" {
		c.DefaultNamespace = "default"
	}
	if !validLabel(c.DefaultNamespace, 63) {
		return c, invalid("default_namespace is not a valid namespace name")
	}
	if len(c.Namespaces) > maxNamespaces {
		return c, invalid("namespaces may list at most %d namespaces", maxNamespaces)
	}
	var namespaces []string
	for _, ns := range c.Namespaces {
		ns = strings.TrimSpace(ns)
		if !validLabel(ns, 63) {
			return c, invalid("namespaces: %q is not a valid namespace name", ns)
		}
		if !slices.Contains(namespaces, ns) {
			namespaces = append(namespaces, ns)
		}
	}
	slices.Sort(namespaces)
	c.Namespaces = namespaces
	if c.Namespaces == nil {
		c.Namespaces = []string{}
	}
	return c, nil
}

// cleanApplication validates the declared fields of an application.
func cleanApplication(a store.Application, cluster store.Cluster) (store.Application, error) {
	a.Name = strings.TrimSpace(a.Name)
	if !validLabel(a.Name, 53) {
		return a, invalid("name must be lowercase letters, digits and '-', at most 53 characters")
	}
	a.ProjectID = strings.TrimSpace(a.ProjectID)
	if a.ProjectID == "" || !printable(a.ProjectID, 100) {
		return a, invalid("project_id is required")
	}
	a.Namespace = strings.TrimSpace(a.Namespace)
	if a.Namespace == "" {
		a.Namespace = cluster.DefaultNamespace
	}
	if !validLabel(a.Namespace, 63) {
		return a, invalid("namespace is not a valid namespace name")
	}
	if len(cluster.Namespaces) > 0 && !slices.Contains(cluster.Namespaces, a.Namespace) {
		return a, invalid("cluster %s only manages the namespaces %s", cluster.Name, strings.Join(cluster.Namespaces, ", "))
	}
	src, err := cleanSource(a.Source)
	if err != nil {
		return a, err
	}
	a.Source = src
	if a.Policy.RetryLimit > maxRetryLimit {
		return a, invalid("sync_policy.retry_limit must be at most %d", maxRetryLimit)
	}
	return a, nil
}

func cleanSource(s store.Source) (store.Source, error) {
	s.Repo = strings.TrimSpace(s.Repo)
	if s.Repo != "" && !github.ValidRepo(s.Repo) {
		return s, invalid("source.repo must be owner/name")
	}
	s.Revision = strings.TrimSpace(s.Revision)
	if !printable(s.Revision, 255) || strings.ContainsAny(s.Revision, " ~^:?*[\\") {
		return s, invalid("source.revision must be a branch, tag or commit SHA")
	}
	p, err := render.CleanPath(s.Path)
	if err != nil {
		return s, invalid("source.path: %s", err.Error())
	}
	s.Path = p
	s.Type = strings.TrimSpace(s.Type)
	switch s.Type {
	case "", render.TypeDirectory, render.TypeKustomize, render.TypeHelm:
	default:
		return s, invalid("source.type must be directory, kustomize, helm or empty (detect)")
	}
	if len(s.HelmValueFiles) > maxValueFiles {
		return s, invalid("source.helm_value_files may list at most %d files", maxValueFiles)
	}
	for i, f := range s.HelmValueFiles {
		f = strings.TrimSpace(f)
		if f == "" || !printable(f, 255) || strings.HasPrefix(f, "/") || strings.Contains(f, "://") {
			return s, invalid("source.helm_value_files: %q must be a file in the repository, relative to the path", f)
		}
		s.HelmValueFiles[i] = f
	}
	if len(s.HelmParameters) > maxHelmParameters {
		return s, invalid("source.helm_parameters may hold at most %d parameters", maxHelmParameters)
	}
	for k, v := range s.HelmParameters {
		if strings.TrimSpace(k) == "" || !printable(k, 255) || len(v) > 4096 {
			return s, invalid("source.helm_parameters: %q is not a valid parameter", k)
		}
	}
	s.HelmReleaseName = strings.TrimSpace(s.HelmReleaseName)
	if s.HelmReleaseName != "" {
		if err := render.ValidReleaseName(s.HelmReleaseName); err != nil {
			return s, invalid("source.helm_release_name: %s", err.Error())
		}
	}
	if len(s.KustomizeImages) > maxImages {
		return s, invalid("source.kustomize_images may list at most %d overrides", maxImages)
	}
	for i, img := range s.KustomizeImages {
		if _, err := render.ParseImage(img); err != nil {
			return s, invalid("source.kustomize_images: %s", err.Error())
		}
		s.KustomizeImages[i] = strings.TrimSpace(img)
	}
	return s, nil
}

func cleanOverrides(images []string, params map[string]string) error {
	if len(images) > maxImages {
		return invalid("kustomize_images may list at most %d overrides", maxImages)
	}
	for _, img := range images {
		if _, err := render.ParseImage(img); err != nil {
			return invalid("kustomize_images: %s", err.Error())
		}
	}
	if len(params) > maxHelmParameters {
		return invalid("helm_parameters may hold at most %d parameters", maxHelmParameters)
	}
	for k, v := range params {
		if strings.TrimSpace(k) == "" || !printable(k, 255) || len(v) > 4096 {
			return invalid("helm_parameters: %q is not a valid parameter", k)
		}
	}
	return nil
}

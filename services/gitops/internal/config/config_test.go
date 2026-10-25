package config

import (
	"testing"
	"time"
)

func TestDefaultsAndTheReconcileMinimum(t *testing.T) {
	for _, k := range []string{"GITOPS_LISTEN", "GITOPS_RECONCILE_SECONDS", "GITOPS_MAX_CLUSTERS", "GITHUB_API_URL", "GITHUB_TOKEN", "SDI_ALLOW_PRIVATE_DEPLOY_TARGETS"} {
		t.Setenv(k, "")
	}
	s, err := FromEnv()
	if err != nil {
		t.Fatal(err)
	}
	if s.Listen != "0.0.0.0:50055" || s.Reconcile != 180*time.Second || s.MaxClusters != 64 ||
		s.GithubAPIURL != "https://api.github.com" || s.Net.AllowPrivate {
		t.Fatalf("defaults %+v", s)
	}
	t.Setenv("GITOPS_RECONCILE_SECONDS", "5")
	if s, _ = FromEnv(); s.Reconcile != 30*time.Second {
		t.Fatalf("minimum 30: %s", s.Reconcile)
	}
	t.Setenv("GITOPS_RECONCILE_SECONDS", "0")
	if s, _ = FromEnv(); s.Reconcile != 0 {
		t.Fatalf("0 turns it off: %s", s.Reconcile)
	}
	t.Setenv("GITOPS_RECONCILE_SECONDS", "-1")
	if _, err := FromEnv(); err == nil {
		t.Fatal("negative accepted")
	}
	t.Setenv("GITOPS_RECONCILE_SECONDS", "")
	t.Setenv("GITOPS_MAX_CLUSTERS", "0")
	if _, err := FromEnv(); err == nil {
		t.Fatal("zero clusters accepted")
	}
	t.Setenv("GITOPS_MAX_CLUSTERS", "")
	t.Setenv("GITHUB_API_URL", "https://ghe.example.com/api/v3/")
	t.Setenv("SDI_ALLOW_PRIVATE_DEPLOY_TARGETS", "1")
	if s, _ = FromEnv(); s.GithubAPIURL != "https://ghe.example.com/api/v3" || !s.Net.AllowPrivate {
		t.Fatalf("GHES base and private targets: %+v", s)
	}
}

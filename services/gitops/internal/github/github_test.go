package github

import (
	"archive/tar"
	"bytes"
	"compress/gzip"
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"strings"
	"sync/atomic"
	"testing"

	"github.com/google/uuid"
)

type entry struct {
	name     string
	body     string
	typeflag byte
	link     string
	size     int64
}

func tarball(t *testing.T, entries []entry) []byte {
	t.Helper()
	var buf bytes.Buffer
	gz := gzip.NewWriter(&buf)
	tw := tar.NewWriter(gz)
	for _, e := range entries {
		flag := e.typeflag
		if flag == 0 {
			flag = tar.TypeReg
		}
		size := int64(len(e.body))
		if e.size > 0 {
			size = e.size
		}
		hdr := &tar.Header{Name: e.name, Mode: 0o644, Size: size, Typeflag: flag, Linkname: e.link}
		if flag != tar.TypeReg {
			hdr.Size = 0
		}
		if flag == tar.TypeXGlobalHeader {
			hdr = &tar.Header{Name: e.name, Typeflag: flag, PAXRecords: map[string]string{"comment": "abc"}}
		}
		if err := tw.WriteHeader(hdr); err != nil {
			t.Fatal(err)
		}
		if flag == tar.TypeReg {
			body := []byte(e.body)
			if e.size > 0 {
				body = bytes.Repeat([]byte("x"), int(e.size))
			}
			if _, err := tw.Write(body); err != nil {
				t.Fatal(err)
			}
		}
	}
	if err := tw.Close(); err != nil {
		t.Fatal(err)
	}
	if err := gz.Close(); err != nil {
		t.Fatal(err)
	}
	return buf.Bytes()
}

func TestExtractKeepsEverythingInsideTheCheckout(t *testing.T) {
	dir := t.TempDir()
	data := tarball(t, []entry{
		{name: "acme-deploy-abc/", typeflag: tar.TypeDir},
		{name: "acme-deploy-abc/app/cm.yaml", body: "kind: ConfigMap"},
		{name: "acme-deploy-abc/../../escape.txt", body: "no"},
		{name: "acme-deploy-abc/app/link", typeflag: tar.TypeSymlink, link: "/etc/passwd"},
		{name: "acme-deploy-abc/app/hard", typeflag: tar.TypeLink, link: "acme-deploy-abc/app/cm.yaml"},
		{name: "acme-deploy-abc/assets/huge.bin", size: 2048},
		{name: "pax_global_header", typeflag: tar.TypeXGlobalHeader},
	})
	limits := DefaultLimits
	limits.MaxFileBytes = 1024
	out := &Checkout{Dir: dir}
	if err := Extract(bytes.NewReader(data), dir, limits, out); err != nil {
		t.Fatal(err)
	}
	if b, err := os.ReadFile(filepath.Join(dir, "app", "cm.yaml")); err != nil || string(b) != "kind: ConfigMap" {
		t.Fatalf("file not extracted: %v", err)
	}
	for _, p := range []string{"app/link", "app/hard", "assets/huge.bin", "../escape.txt", "../../escape.txt"} {
		if _, err := os.Lstat(filepath.Join(dir, p)); err == nil {
			t.Errorf("%s must not be extracted", p)
		}
	}
	if len(out.Oversized) != 1 || out.Oversized[0] != "assets/huge.bin" {
		t.Errorf("oversized = %v", out.Oversized)
	}
}

func TestExtractEnforcesTheLimits(t *testing.T) {
	data := tarball(t, []entry{{name: "r/a.txt", size: 4096}, {name: "r/b.txt", size: 4096}})
	limits := DefaultLimits
	limits.MaxTarballBytes = 100
	err := Extract(bytes.NewReader(data), t.TempDir(), limits, &Checkout{})
	if err == nil || !strings.Contains(err.Error(), "tarball is larger") {
		t.Fatalf("tarball limit: %v", err)
	}
	limits = DefaultLimits
	limits.MaxExtractedBytes = 5000
	err = Extract(bytes.NewReader(data), t.TempDir(), limits, &Checkout{})
	if err == nil || !strings.Contains(err.Error(), "unpacks to more") {
		t.Fatalf("extracted limit: %v", err)
	}
	limits = DefaultLimits
	limits.MaxFiles = 1
	err = Extract(bytes.NewReader(data), t.TempDir(), limits, &Checkout{})
	if err == nil || !strings.Contains(err.Error(), "more than 1 files") {
		t.Fatalf("file count limit: %v", err)
	}
	if err := Extract(strings.NewReader("not gzip"), t.TempDir(), DefaultLimits, &Checkout{}); err == nil {
		t.Fatal("garbage extracted")
	}
}

type staticTokens struct{ access Access }

func (s staticTokens) Access(context.Context, uuid.UUID) (Access, error) { return s.access, nil }

const sha = "0123456789abcdef0123456789abcdef01234567"

func TestTheAPIResolvesRevisionsAndDownloadsThroughTheRedirect(t *testing.T) {
	archive := tarball(t, []entry{{name: "acme-deploy-0123456/app/cm.yaml", body: "kind: ConfigMap"}})
	var authOnCodeload atomic.Value
	mux := http.NewServeMux()
	var api *httptest.Server
	mux.HandleFunc("/repos/acme/deploy", func(w http.ResponseWriter, r *http.Request) {
		if r.Header.Get("Authorization") != "Bearer tok" {
			w.WriteHeader(http.StatusUnauthorized)
			return
		}
		_ = json.NewEncoder(w).Encode(map[string]string{"default_branch": "main"})
	})
	mux.HandleFunc("/repos/acme/deploy/commits/", func(w http.ResponseWriter, r *http.Request) {
		switch strings.TrimPrefix(r.URL.Path, "/repos/acme/deploy/commits/") {
		case "main", "v1.0", sha:
			_ = json.NewEncoder(w).Encode(map[string]string{"sha": sha})
		default:
			w.WriteHeader(http.StatusNotFound)
		}
	})
	mux.HandleFunc("/repos/acme/deploy/tarball/"+sha, func(w http.ResponseWriter, r *http.Request) {
		http.Redirect(w, r, api.URL+"/codeload/acme/deploy/"+sha, http.StatusFound)
	})
	mux.HandleFunc("/codeload/acme/deploy/"+sha, func(w http.ResponseWriter, r *http.Request) {
		authOnCodeload.Store(r.Header.Get("Authorization"))
		_, _ = w.Write(archive)
	})
	api = httptest.NewServer(mux)
	defer api.Close()

	c := New(staticTokens{Access{Token: "tok", APIBase: api.URL}}, DefaultLimits)
	ctx := context.Background()
	ws := uuid.New()
	if b, err := c.DefaultBranch(ctx, ws, "acme/deploy"); err != nil || b != "main" {
		t.Fatalf("default branch %q, %v", b, err)
	}
	for _, rev := range []string{"main", "v1.0"} {
		if got, err := c.ResolveCommit(ctx, ws, "acme/deploy", rev); err != nil || got != sha {
			t.Fatalf("resolve %s: %q, %v", rev, got, err)
		}
	}
	_, err := c.ResolveCommit(ctx, ws, "acme/deploy", "nope")
	var gerr *Error
	if err == nil || !strings.Contains(err.Error(), `"nope"`) {
		t.Fatalf("unknown revision: %v", err)
	}
	if e, ok := err.(*Error); ok {
		gerr = e
	}
	if gerr == nil || !gerr.NotFound {
		t.Fatalf("unknown revision is NotFound: %v", err)
	}
	checkout, err := c.Download(ctx, ws, "acme/deploy", sha)
	if err != nil {
		t.Fatal(err)
	}
	defer checkout.Remove()
	if b, err := os.ReadFile(filepath.Join(checkout.Dir, "app", "cm.yaml")); err != nil || string(b) != "kind: ConfigMap" {
		t.Fatalf("checkout: %v", err)
	}
	checkout.Remove()
	if _, err := os.Stat(checkout.Dir); !os.IsNotExist(err) {
		t.Fatal("Remove must delete the checkout")
	}
	for _, bad := range []string{"../etc", "acme", "acme/deploy/x", "a b/c"} {
		if _, err := c.ResolveCommit(ctx, ws, bad, "main"); err == nil {
			t.Errorf("repo %q accepted", bad)
		}
	}
	if _, err := New(staticTokens{Access{Token: "wrong", APIBase: api.URL}}, DefaultLimits).DefaultBranch(ctx, ws, "acme/deploy"); err == nil || !strings.Contains(err.Error(), "rejected the workspace's token") {
		t.Fatalf("a rejected token: %v", err)
	}
	_ = authOnCodeload.Load()
}

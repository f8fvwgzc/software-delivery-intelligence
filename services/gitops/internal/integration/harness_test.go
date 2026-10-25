// Package integration runs the gitops service end to end against a real
// Kubernetes cluster and Postgres: the gRPC API in process, identity and
// knowledge faked as gRPC servers, GitHub faked as an HTTP server serving
// tarballs of the fixtures under testdata/repo. It runs only when both
// DATABASE_URL and SDI_GITOPS_TEST_KUBECONFIG are set, uses that kubeconfig
// and nothing else, and every test works in a namespace
// sdi-gitops-test-<random> that it deletes at the end.
package integration

import (
	"archive/tar"
	"bytes"
	"compress/gzip"
	"context"
	"crypto/rand"
	"crypto/sha1"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"io"
	"io/fs"
	"log/slog"
	"net"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"regexp"
	"sort"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/google/uuid"
	"github.com/jackc/pgx/v5"
	"google.golang.org/grpc"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/credentials/insecure"
	"google.golang.org/grpc/status"
	"google.golang.org/grpc/test/bufconn"
	"google.golang.org/protobuf/types/known/timestamppb"
	apierrors "k8s.io/apimachinery/pkg/api/errors"
	metav1 "k8s.io/apimachinery/pkg/apis/meta/v1"
	"k8s.io/apimachinery/pkg/types"
	"k8s.io/client-go/kubernetes"
	"k8s.io/client-go/tools/clientcmd"

	gitopsv1 "github.com/f8fvwgzc/software-delivery-intelligence/services/gitops/gen/sdi/gitops/v1"
	identityv1 "github.com/f8fvwgzc/software-delivery-intelligence/services/gitops/gen/sdi/identity/v1"
	knowledgev1 "github.com/f8fvwgzc/software-delivery-intelligence/services/gitops/gen/sdi/knowledge/v1"
	"github.com/f8fvwgzc/software-delivery-intelligence/services/gitops/internal/clusters"
	"github.com/f8fvwgzc/software-delivery-intelligence/services/gitops/internal/config"
	"github.com/f8fvwgzc/software-delivery-intelligence/services/gitops/internal/db"
	"github.com/f8fvwgzc/software-delivery-intelligence/services/gitops/internal/events"
	"github.com/f8fvwgzc/software-delivery-intelligence/services/gitops/internal/github"
	"github.com/f8fvwgzc/software-delivery-intelligence/services/gitops/internal/gitops"
	"github.com/f8fvwgzc/software-delivery-intelligence/services/gitops/internal/logx"
	"github.com/f8fvwgzc/software-delivery-intelligence/services/gitops/internal/netguard"
	"github.com/f8fvwgzc/software-delivery-intelligence/services/gitops/internal/peers"
	"github.com/f8fvwgzc/software-delivery-intelligence/services/gitops/internal/principal"
	"github.com/f8fvwgzc/software-delivery-intelligence/services/gitops/internal/server"
	"github.com/f8fvwgzc/software-delivery-intelligence/services/gitops/internal/store"
)

const (
	repoName      = "acme/deploy"
	kubeSecret    = "TEST_KUBECONFIG"
	githubToken   = "gh-test-token"
	nsPrefix      = "sdi-gitops-test-"
	healthTimeout = 3 * time.Minute
)

func randomHex(n int) string {
	b := make([]byte, n)
	_, _ = rand.Read(b)
	return hex.EncodeToString(b)
}

// --- Fake GitHub ------------------------------------------------------------------------

// fakeGitHub serves one repository whose commits are snapshots of the
// fixture tree: the commit API and the tarball endpoint (redirecting to a
// codeload path, as GitHub does).
type fakeGitHub struct {
	srv     *httptest.Server
	mu      sync.Mutex
	commits map[string]map[string][]byte
	head    string
	bad     []string
}

func newFakeGitHub(t *testing.T) *fakeGitHub {
	g := &fakeGitHub{commits: map[string]map[string][]byte{}}
	files := map[string][]byte{}
	root := filepath.Join("testdata", "repo")
	err := filepath.WalkDir(root, func(p string, d fs.DirEntry, err error) error {
		if err != nil || d.IsDir() {
			return err
		}
		data, err := os.ReadFile(p)
		if err != nil {
			return err
		}
		rel, _ := filepath.Rel(root, p)
		files[filepath.ToSlash(rel)] = data
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
	g.store(files)
	mux := http.NewServeMux()
	mux.HandleFunc("/", g.serve)
	g.srv = httptest.NewServer(mux)
	t.Cleanup(g.srv.Close)
	return g
}

func (g *fakeGitHub) store(files map[string][]byte) string {
	h := sha1.New()
	names := make([]string, 0, len(files))
	for n := range files {
		names = append(names, n)
	}
	sort.Strings(names)
	for _, n := range names {
		fmt.Fprintf(h, "%s\x00%s\x00", n, files[n])
	}
	sha := hex.EncodeToString(h.Sum(nil))
	g.commits[sha] = files
	g.head = sha
	return sha
}

// commit makes a new commit on main from the current head.
func (g *fakeGitHub) commit(change func(files map[string][]byte)) string {
	g.mu.Lock()
	defer g.mu.Unlock()
	files := map[string][]byte{}
	for k, v := range g.commits[g.head] {
		files[k] = append([]byte(nil), v...)
	}
	change(files)
	return g.store(files)
}

func (g *fakeGitHub) headSHA() string {
	g.mu.Lock()
	defer g.mu.Unlock()
	return g.head
}

var commitPath = regexp.MustCompile(`^/repos/acme/deploy/commits/([^/]+)$`)
var tarballPath = regexp.MustCompile(`^/repos/acme/deploy/tarball/([0-9a-f]{40})$`)
var codeloadPath = regexp.MustCompile(`^/codeload/acme/deploy/([0-9a-f]{40})$`)

func (g *fakeGitHub) serve(w http.ResponseWriter, r *http.Request) {
	if m := codeloadPath.FindStringSubmatch(r.URL.Path); m != nil {
		g.mu.Lock()
		files, ok := g.commits[m[1]]
		g.mu.Unlock()
		if !ok {
			http.NotFound(w, r)
			return
		}
		w.Header().Set("Content-Type", "application/x-gzip")
		_, _ = w.Write(tarGz("acme-deploy-"+m[1][:7], files))
		return
	}
	if r.Header.Get("Authorization") != "Bearer "+githubToken {
		g.mu.Lock()
		g.bad = append(g.bad, r.URL.Path)
		g.mu.Unlock()
		w.WriteHeader(http.StatusUnauthorized)
		return
	}
	switch {
	case r.URL.Path == "/repos/acme/deploy":
		_ = json.NewEncoder(w).Encode(map[string]string{"default_branch": "main", "full_name": repoName})
	case commitPath.MatchString(r.URL.Path):
		ref := commitPath.FindStringSubmatch(r.URL.Path)[1]
		g.mu.Lock()
		sha := ""
		if ref == "main" {
			sha = g.head
		} else if _, ok := g.commits[ref]; ok {
			sha = ref
		}
		g.mu.Unlock()
		if sha == "" {
			w.WriteHeader(http.StatusNotFound)
			return
		}
		_ = json.NewEncoder(w).Encode(map[string]string{"sha": sha})
	case tarballPath.MatchString(r.URL.Path):
		sha := tarballPath.FindStringSubmatch(r.URL.Path)[1]
		http.Redirect(w, r, g.srv.URL+"/codeload/acme/deploy/"+sha, http.StatusFound)
	default:
		http.NotFound(w, r)
	}
}

func tarGz(top string, files map[string][]byte) []byte {
	var buf bytes.Buffer
	gz := gzip.NewWriter(&buf)
	tw := tar.NewWriter(gz)
	names := make([]string, 0, len(files))
	for n := range files {
		names = append(names, n)
	}
	sort.Strings(names)
	_ = tw.WriteHeader(&tar.Header{Name: top + "/", Typeflag: tar.TypeDir, Mode: 0o755})
	for _, n := range names {
		_ = tw.WriteHeader(&tar.Header{Name: top + "/" + n, Typeflag: tar.TypeReg, Mode: 0o644, Size: int64(len(files[n]))})
		_, _ = tw.Write(files[n])
	}
	_ = tw.Close()
	_ = gz.Close()
	return buf.Bytes()
}

// --- Fake identity and knowledge ----------------------------------------------------------

var secretName = regexp.MustCompile(`^[A-Z][A-Z0-9_]{1,63}$`)

// fakeIdentity applies identity's own rules to ResolveSecret
// (services/identity/src/grpc/secrets.rs): the OWNER role, the body's
// workspace equal to the caller's, an upper-snake-case name.
type fakeIdentity struct {
	identityv1.UnimplementedIdentityServiceServer
	mu        sync.Mutex
	secrets   map[string]map[string]string
	githubURL string
	audits    []*identityv1.AuditEntry
	resolves  []string
}

func (f *fakeIdentity) ResolveSecret(ctx context.Context, req *identityv1.ResolveSecretRequest) (*identityv1.ResolvedSecret, error) {
	p, err := principal.FromIncoming(ctx)
	if err != nil {
		return nil, err
	}
	if err := p.Require(principal.Owner); err != nil {
		return nil, err
	}
	if req.GetWorkspaceId() != p.WorkspaceID.String() {
		return nil, status.Error(codes.PermissionDenied, "workspace_id does not match the caller's workspace")
	}
	if !secretName.MatchString(req.GetName()) {
		return nil, status.Error(codes.InvalidArgument, "name must be upper snake case")
	}
	f.mu.Lock()
	defer f.mu.Unlock()
	f.resolves = append(f.resolves, p.UserID+"/"+req.GetName())
	value, ok := f.secrets[req.GetWorkspaceId()][req.GetName()]
	if !ok {
		return nil, status.Errorf(codes.NotFound, "no secret named %s", req.GetName())
	}
	return &identityv1.ResolvedSecret{Value: value}, nil
}

func (f *fakeIdentity) GetInstallationToken(ctx context.Context, req *identityv1.GetInstallationTokenRequest) (*identityv1.InstallationToken, error) {
	if _, err := principal.FromIncoming(ctx); err != nil {
		return nil, err
	}
	return &identityv1.InstallationToken{Token: githubToken, ApiBaseUrl: f.githubURL,
		ExpiresAt: timestamppb.New(time.Now().Add(time.Hour))}, nil
}

func (f *fakeIdentity) RecordAudit(_ context.Context, req *identityv1.RecordAuditRequest) (*identityv1.RecordAuditResponse, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	f.audits = append(f.audits, req.GetEntry())
	return &identityv1.RecordAuditResponse{}, nil
}

func (f *fakeIdentity) audited(action string) []*identityv1.AuditEntry {
	f.mu.Lock()
	defer f.mu.Unlock()
	var out []*identityv1.AuditEntry
	for _, a := range f.audits {
		if a.GetAction() == action {
			out = append(out, a)
		}
	}
	return out
}

type fakeKnowledge struct {
	knowledgev1.UnimplementedKnowledgeServiceServer
	workspace uuid.UUID
	projectID string
}

func (f *fakeKnowledge) GetProject(ctx context.Context, req *knowledgev1.GetProjectRequest) (*knowledgev1.Project, error) {
	p, err := principal.FromIncoming(ctx)
	if err != nil {
		return nil, err
	}
	if p.WorkspaceID != f.workspace || req.GetId() != f.projectID {
		return nil, status.Error(codes.NotFound, "no such project")
	}
	return &knowledgev1.Project{Id: f.projectID, Name: "deploy", RepoFullName: repoName, DefaultBranch: "main"}, nil
}

// serveBuf runs a gRPC server on an in-memory listener.
func serveBuf(t *testing.T, register func(*grpc.Server)) *grpc.ClientConn {
	return serveBufWith(t, register)
}

// --- The environment -----------------------------------------------------------------------

type waveLog struct {
	mu    sync.Mutex
	waves map[uuid.UUID][]string
}

func (w *waveLog) record(app uuid.UUID, phase string, wave int) {
	w.mu.Lock()
	defer w.mu.Unlock()
	w.waves[app] = append(w.waves[app], fmt.Sprintf("%s/%d", phase, wave))
}

func (w *waveLog) of(app string) []string {
	w.mu.Lock()
	defer w.mu.Unlock()
	return append([]string(nil), w.waves[uuid.MustParse(app)]...)
}

type env struct {
	t          *testing.T
	workspace  uuid.UUID
	projectID  string
	client     gitopsv1.GitopsServiceClient
	kube       kubernetes.Interface
	kubeconfig []byte
	github     *fakeGitHub
	identity   *fakeIdentity
	svc        *gitops.Service
	reconciler *gitops.Reconciler
	waves      *waveLog
	store      *store.Store
}

func newEnv(t *testing.T) *env {
	t.Helper()
	dbURL := os.Getenv("DATABASE_URL")
	kubeconfigPath := os.Getenv("SDI_GITOPS_TEST_KUBECONFIG")
	if dbURL == "" || kubeconfigPath == "" {
		t.Skip("cluster integration tests need DATABASE_URL and SDI_GITOPS_TEST_KUBECONFIG")
	}
	if os.Getenv(netguard.AllowPrivateEnv) != "1" {
		t.Skipf("the test cluster's API is on 127.0.0.1: set %s=1", netguard.AllowPrivateEnv)
	}
	raw, err := os.ReadFile(kubeconfigPath)
	if err != nil {
		t.Fatalf("kubeconfig: %v", err)
	}
	// The assertions use exactly this file's current context, nothing else.
	restConfig, err := clientcmd.RESTConfigFromKubeConfig(raw)
	if err != nil {
		t.Fatalf("kubeconfig: %v", err)
	}
	kube, err := kubernetes.NewForConfig(restConfig)
	if err != nil {
		t.Fatal(err)
	}

	schema := "gitops_it_" + randomHex(6)
	ctx := context.Background()
	pool, err := db.Connect(ctx, dbURL, schema, config.DefaultPool())
	if err != nil {
		t.Fatalf("database: %v", err)
	}
	t.Cleanup(func() {
		pool.Close()
		if conn, err := pgx.Connect(context.Background(), dbURL); err == nil {
			_, _ = conn.Exec(context.Background(), "DROP SCHEMA "+schema+" CASCADE")
			_ = conn.Close(context.Background())
		}
	})

	gh := newFakeGitHub(t)
	workspace := uuid.New()
	projectID := uuid.NewString()
	ident := &fakeIdentity{githubURL: gh.srv.URL, secrets: map[string]map[string]string{
		workspace.String(): {kubeSecret: string(raw)},
	}}
	identityConn := serveBuf(t, func(s *grpc.Server) { identityv1.RegisterIdentityServiceServer(s, ident) })
	knowledgeConn := serveBuf(t, func(s *grpc.Server) {
		knowledgev1.RegisterKnowledgeServiceServer(s, &fakeKnowledge{workspace: workspace, projectID: projectID})
	})

	log := slog.New(slog.NewTextHandler(io.Discard, nil))
	if os.Getenv("SDI_GITOPS_TEST_VERBOSE") != "" {
		log = slog.New(slog.NewTextHandler(os.Stderr, &slog.HandlerOptions{Level: slog.LevelDebug}))
	}
	identity := peers.NewIdentity(identityConn, log)
	tokens := peers.NewTokens(identity, "", "", config.DefaultGithubAPI, log)
	engineLog := logx.Engine(log)
	manager := clusters.NewManager(identity, netguard.New(netguard.Policy{AllowPrivate: true}), 8, log, engineLog)
	shutdown, cancel := context.WithCancel(context.Background())
	t.Cleanup(cancel)
	waves := &waveLog{waves: map[uuid.UUID][]string{}}
	st := store.New(pool)
	svc := gitops.New(gitops.Options{
		Store: st, Clusters: manager, Repos: github.New(tokens, github.DefaultLimits),
		Projects: peers.NewKnowledge(knowledgeConn), Audit: identity, Events: events.NewHub(),
		Log: log, EngineLog: engineLog, Shutdown: shutdown, SyncWaveDelay: 500 * time.Millisecond,
	})
	svc.WaveObserver = waves.record
	api := server.New(svc, log)
	unary, streaming := server.Interceptors(log)
	conn := serveBufWith(t, func(s *grpc.Server) { gitopsv1.RegisterGitopsServiceServer(s, api) },
		grpc.ChainUnaryInterceptor(unary), grpc.ChainStreamInterceptor(streaming))
	t.Cleanup(func() { manager.DropWorkspace(workspace) })
	return &env{t: t, workspace: workspace, projectID: projectID, client: gitopsv1.NewGitopsServiceClient(conn),
		kube: kube, kubeconfig: raw, github: gh, identity: ident, svc: svc, store: st,
		reconciler: gitops.NewReconciler(svc, 0), waves: waves}
}

func serveBufWith(t *testing.T, register func(*grpc.Server), opts ...grpc.ServerOption) *grpc.ClientConn {
	lis := bufconn.Listen(1 << 20)
	opts = append(opts, grpc.MaxRecvMsgSize(peers.MaxMessageBytes), grpc.MaxSendMsgSize(peers.MaxMessageBytes))
	srv := grpc.NewServer(opts...)
	register(srv)
	go func() { _ = srv.Serve(lis) }()
	conn, err := grpc.NewClient("passthrough:///bufnet",
		grpc.WithContextDialer(func(ctx context.Context, _ string) (net.Conn, error) { return lis.DialContext(ctx) }),
		grpc.WithTransportCredentials(insecure.NewCredentials()),
		grpc.WithDefaultCallOptions(grpc.MaxCallRecvMsgSize(peers.MaxMessageBytes)))
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() {
		_ = conn.Close()
		srv.Stop()
	})
	return conn
}

// as is a call context with role in the test workspace.
func (e *env) as(role principal.Role) context.Context {
	ctx := principal.Principal{UserID: "user-" + role.String(), WorkspaceID: e.workspace, Role: role}.Outgoing(context.Background())
	ctx, cancel := context.WithTimeout(ctx, 10*time.Minute)
	e.t.Cleanup(cancel)
	return ctx
}

// namespace is a fresh namespace name, deleted (with everything in it) at
// the end of the test. Only names with the test prefix are ever deleted.
func (e *env) namespace() string {
	name := nsPrefix + randomHex(4)
	e.t.Cleanup(func() {
		if !strings.HasPrefix(name, nsPrefix) {
			return
		}
		ctx, cancel := context.WithTimeout(context.Background(), 2*time.Minute)
		defer cancel()
		// A test that failed mid-sync may leave the engine's hook finalizer
		// on a Job, which would keep the namespace terminating forever.
		if jobs, err := e.kube.BatchV1().Jobs(name).List(ctx, metav1.ListOptions{}); err == nil {
			for _, j := range jobs.Items {
				if len(j.Finalizers) > 0 {
					patch := []byte(`[{"op":"remove","path":"/metadata/finalizers"}]`)
					_, _ = e.kube.BatchV1().Jobs(name).Patch(ctx, j.Name, types.JSONPatchType, patch, metav1.PatchOptions{})
				}
			}
		}
		err := e.kube.CoreV1().Namespaces().Delete(ctx, name, metav1.DeleteOptions{})
		if err != nil && !apierrors.IsNotFound(err) {
			e.t.Logf("could not delete namespace %s: %v", name, err)
			return
		}
		for ctx.Err() == nil {
			if _, err := e.kube.CoreV1().Namespaces().Get(ctx, name, metav1.GetOptions{}); apierrors.IsNotFound(err) {
				return
			}
			time.Sleep(time.Second)
		}
		e.t.Logf("namespace %s still terminating", name)
	})
	return name
}

func (e *env) cluster() *gitopsv1.Cluster {
	e.t.Helper()
	c, err := e.client.UpsertCluster(e.as(principal.Admin), &gitopsv1.UpsertClusterRequest{Cluster: &gitopsv1.Cluster{
		Name: "test-" + randomHex(3), KubeconfigSecret: kubeSecret,
	}})
	if err != nil {
		e.t.Fatalf("UpsertCluster: %v", err)
	}
	if c.GetStatus() != "connected" {
		e.t.Fatalf("the test cluster is %s: %s", c.GetStatus(), c.GetMessage())
	}
	return c
}

func (e *env) app(cluster *gitopsv1.Cluster, ns string, source *gitopsv1.ApplicationSource, policy *gitopsv1.SyncPolicy) *gitopsv1.Application {
	e.t.Helper()
	a, err := e.client.UpsertApplication(e.as(principal.Admin), &gitopsv1.UpsertApplicationRequest{Application: &gitopsv1.Application{
		ProjectId: e.projectID, Name: "app-" + randomHex(3), ClusterId: cluster.GetId(), Namespace: ns,
		Source: source, SyncPolicy: policy,
	}})
	if err != nil {
		e.t.Fatalf("UpsertApplication: %v", err)
	}
	return a
}

func (e *env) refresh(id string) *gitopsv1.Application {
	e.t.Helper()
	a, err := e.client.RefreshApplication(e.as(principal.Member), &gitopsv1.RefreshApplicationRequest{Id: id})
	if err != nil {
		e.t.Fatalf("RefreshApplication: %v", err)
	}
	return a
}

func (e *env) sync(req *gitopsv1.SyncApplicationRequest) *gitopsv1.SyncResult {
	e.t.Helper()
	res, err := e.client.SyncApplication(e.as(principal.Approver), req)
	if err != nil {
		e.t.Fatalf("SyncApplication: %v", err)
	}
	return res
}

// eventually polls cond until it holds or timeout passes.
func (e *env) eventually(timeout time.Duration, what string, cond func() (bool, string)) {
	e.t.Helper()
	deadline := time.Now().Add(timeout)
	last := ""
	for time.Now().Before(deadline) {
		ok, detail := cond()
		if ok {
			return
		}
		last = detail
		time.Sleep(time.Second)
	}
	e.t.Fatalf("%s: not reached within %s (last: %s)", what, timeout, last)
}

func (e *env) waitHealthy(id string) *gitopsv1.Application {
	e.t.Helper()
	var app *gitopsv1.Application
	e.eventually(healthTimeout, "application healthy", func() (bool, string) {
		app = e.refresh(id)
		return app.GetHealthStatus() == "healthy" && app.GetSyncStatus() == "synced",
			fmt.Sprintf("%s/%s %s %v", app.GetSyncStatus(), app.GetHealthStatus(), app.GetHealthMessage(), app.GetConditions())
	})
	return app
}

func resourceOf(app *gitopsv1.Application, kind, name string) *gitopsv1.ResourceStatus {
	for _, r := range app.GetResources() {
		if r.GetKind() == kind && r.GetName() == name {
			return r
		}
	}
	return nil
}

func resultOf(res *gitopsv1.SyncResult, kind, name string) *gitopsv1.ResourceResult {
	for _, r := range res.GetResources() {
		if r.GetKind() == kind && r.GetName() == name {
			return r
		}
	}
	return nil
}

func describeResults(res *gitopsv1.SyncResult) string {
	var parts []string
	for _, r := range res.GetResources() {
		parts = append(parts, fmt.Sprintf("%s/%s=%s(%s)", r.GetKind(), r.GetName(), r.GetStatus(), r.GetMessage()))
	}
	return res.GetPhase() + " " + res.GetMessage() + " [" + strings.Join(parts, ", ") + "]"
}

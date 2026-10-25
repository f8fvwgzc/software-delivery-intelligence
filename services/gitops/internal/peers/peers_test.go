package peers

import (
	"context"
	"io"
	"log/slog"
	"net"
	"strings"
	"sync/atomic"
	"testing"
	"time"

	"github.com/google/uuid"
	"google.golang.org/grpc"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/credentials/insecure"
	"google.golang.org/grpc/status"
	"google.golang.org/grpc/test/bufconn"
	"google.golang.org/protobuf/types/known/timestamppb"

	identityv1 "github.com/f8fvwgzc/software-delivery-intelligence/services/gitops/gen/sdi/identity/v1"
	"github.com/f8fvwgzc/software-delivery-intelligence/services/gitops/internal/principal"
)

type identityFake struct {
	identityv1.UnimplementedIdentityServiceServer
	answer error
	minted atomic.Int32
	seen   atomic.Value
}

func (f *identityFake) GetInstallationToken(ctx context.Context, req *identityv1.GetInstallationTokenRequest) (*identityv1.InstallationToken, error) {
	p, _ := principal.FromIncoming(ctx)
	f.seen.Store(p)
	if f.answer != nil {
		return nil, f.answer
	}
	f.minted.Add(1)
	return &identityv1.InstallationToken{Token: "ghs_minted", ApiBaseUrl: "https://ghe.example.com/api/v3/",
		ExpiresAt: timestamppb.New(time.Now().Add(time.Hour))}, nil
}

func (f *identityFake) ResolveSecret(ctx context.Context, req *identityv1.ResolveSecretRequest) (*identityv1.ResolvedSecret, error) {
	p, _ := principal.FromIncoming(ctx)
	f.seen.Store(p)
	return &identityv1.ResolvedSecret{Value: "value-of-" + req.GetName()}, nil
}

func dialFake(t *testing.T, f *identityFake) *grpc.ClientConn {
	lis := bufconn.Listen(1 << 16)
	srv := grpc.NewServer()
	identityv1.RegisterIdentityServiceServer(srv, f)
	go func() { _ = srv.Serve(lis) }()
	conn, err := grpc.NewClient("passthrough:///bufnet",
		grpc.WithContextDialer(func(ctx context.Context, _ string) (net.Conn, error) { return lis.DialContext(ctx) }),
		grpc.WithTransportCredentials(insecure.NewCredentials()))
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = conn.Close(); srv.Stop() })
	return conn
}

var quiet = slog.New(slog.NewTextHandler(io.Discard, nil))

func TestIdentityMintsAndTheTokenIsCached(t *testing.T) {
	f := &identityFake{}
	tokens := NewTokens(NewIdentity(dialFake(t, f), quiet), "ghp_dev", "https://dev.example.com", "https://api.github.com", quiet)
	ws := uuid.New()
	a, err := tokens.Access(context.Background(), ws)
	if err != nil || a.Token != "ghs_minted" || a.APIBase != "https://ghe.example.com/api/v3" {
		t.Fatalf("access %v, %v", a, err)
	}
	if p := f.seen.Load().(principal.Principal); p.UserID != principal.SystemUser || p.WorkspaceID != ws || p.Role != principal.Owner {
		t.Fatalf("minted as %+v, want the workspace's system principal", p)
	}
	if _, err := tokens.Access(context.Background(), ws); err != nil || f.minted.Load() != 1 {
		t.Fatalf("a fresh token is reused: minted %d", f.minted.Load())
	}
	tokens.Forget(ws)
	_, _ = tokens.Access(context.Background(), ws)
	if f.minted.Load() != 2 {
		t.Fatal("Forget drops the cached token")
	}
	if strings.Contains(a.String(), "ghs_minted") {
		t.Fatal("an access prints its token")
	}
}

func TestTheDevTokenOnlyStandsInWhenIdentityIsUnreachable(t *testing.T) {
	// Unreachable: a connection nobody listens on.
	conn, err := Dial("http://127.0.0.1:1")
	if err != nil {
		t.Fatal(err)
	}
	defer conn.Close()
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	tokens := NewTokens(NewIdentity(conn, quiet), "ghp_dev", "https://ghe.example.com/api/v3", "https://api.github.com", quiet)
	a, err := tokens.Access(ctx, uuid.New())
	if err != nil || a.Token != "ghp_dev" || a.APIBase != "https://ghe.example.com/api/v3" {
		t.Fatalf("dev fallback: %v, %v", a, err)
	}
	noDev := NewTokens(NewIdentity(conn, quiet), "", "", "https://api.github.com", quiet)
	if _, err := noDev.Access(ctx, uuid.New()); err == nil || !strings.Contains(err.Error(), "GITHUB_TOKEN is not set") {
		t.Fatalf("no fallback configured: %v", err)
	}
	// Reachable but refusing: no fallback, the refusal stands.
	f := &identityFake{answer: status.Error(codes.FailedPrecondition, "the SDI GitHub App was uninstalled")}
	refusing := NewTokens(NewIdentity(dialFake(t, f), quiet), "ghp_dev", "https://dev.example.com", "https://api.github.com", quiet)
	if _, err := refusing.Access(context.Background(), uuid.New()); err == nil || !strings.Contains(err.Error(), "uninstalled") {
		t.Fatalf("a refusal must not fall back to the dev token: %v", err)
	}
}

func TestSecretsAreResolvedAsTheWorkspacesSystemPrincipal(t *testing.T) {
	f := &identityFake{}
	identity := NewIdentity(dialFake(t, f), quiet)
	ws := uuid.New()
	v, err := identity.ResolveSecret(context.Background(), ws, "PROD_KUBECONFIG")
	if err != nil || v != "value-of-PROD_KUBECONFIG" {
		t.Fatalf("resolve: %q %v", v, err)
	}
	if p := f.seen.Load().(principal.Principal); p.UserID != principal.SystemUser || p.WorkspaceID != ws || p.Role != principal.Owner {
		t.Fatalf("resolved as %+v", p)
	}
	if !Unreachable(status.Error(codes.Unavailable, "")) || Unreachable(status.Error(codes.NotFound, "")) {
		t.Fatal("Unreachable")
	}
}

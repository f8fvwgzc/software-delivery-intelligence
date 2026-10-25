// sdi-gitops is the GitOps engine of SDI (CONTRACT.md §24): Argo CD's
// gitops-engine embedded behind a gRPC API on :50055.
//
//	sdi-gitops              serve
//	sdi-gitops healthcheck  ask the running service's gRPC health endpoint
//	                        (the container healthcheck; distroless has no shell)
package main

import (
	"context"
	"fmt"
	"log/slog"
	"net"
	"os"
	"os/signal"
	"path/filepath"
	"syscall"
	"time"

	"google.golang.org/grpc"
	"google.golang.org/grpc/credentials/insecure"
	"google.golang.org/grpc/health"
	healthpb "google.golang.org/grpc/health/grpc_health_v1"
	"google.golang.org/grpc/keepalive"

	engineio "github.com/argoproj/argo-cd/gitops-engine/pkg/utils/io"

	gitopsv1 "github.com/f8fvwgzc/software-delivery-intelligence/services/gitops/gen/sdi/gitops/v1"
	"github.com/f8fvwgzc/software-delivery-intelligence/services/gitops/internal/clusters"
	"github.com/f8fvwgzc/software-delivery-intelligence/services/gitops/internal/config"
	"github.com/f8fvwgzc/software-delivery-intelligence/services/gitops/internal/db"
	"github.com/f8fvwgzc/software-delivery-intelligence/services/gitops/internal/events"
	"github.com/f8fvwgzc/software-delivery-intelligence/services/gitops/internal/github"
	"github.com/f8fvwgzc/software-delivery-intelligence/services/gitops/internal/gitops"
	"github.com/f8fvwgzc/software-delivery-intelligence/services/gitops/internal/logx"
	"github.com/f8fvwgzc/software-delivery-intelligence/services/gitops/internal/netguard"
	"github.com/f8fvwgzc/software-delivery-intelligence/services/gitops/internal/peers"
	"github.com/f8fvwgzc/software-delivery-intelligence/services/gitops/internal/render"
	"github.com/f8fvwgzc/software-delivery-intelligence/services/gitops/internal/server"
	"github.com/f8fvwgzc/software-delivery-intelligence/services/gitops/internal/store"
)

const serviceName = "sdi.gitops.v1.GitopsService"

func main() {
	if len(os.Args) > 1 && os.Args[1] == "healthcheck" {
		os.Exit(healthcheck())
	}
	log := logx.Setup("gitops")
	if err := run(log); err != nil {
		log.Error("gitops service failed", "error", err)
		os.Exit(1)
	}
}

func run(log *slog.Logger) error {
	settings, err := config.FromEnv()
	if err != nil {
		return err
	}
	leaving, stop := signal.NotifyContext(context.Background(), os.Interrupt, syscall.SIGTERM)
	defer stop()
	// Syncs in flight get the grace period after the signal; then this ends
	// and they are terminated.
	hardStop, cancelHard := context.WithCancel(context.Background())
	defer cancelHard()

	cleanupTmp, err := privateEngineTempDir()
	if err != nil {
		return err
	}
	defer cleanupTmp()
	render.BlockDefaultNetwork()

	startCtx, cancelStart := context.WithTimeout(leaving, time.Minute)
	pool, err := db.Connect(startCtx, settings.DatabaseURL, db.Schema, settings.Pool)
	cancelStart()
	if err != nil {
		return err
	}
	defer pool.Close()

	identityConn, err := peers.Dial(settings.IdentityURL)
	if err != nil {
		return err
	}
	defer identityConn.Close()
	knowledgeConn, err := peers.Dial(settings.KnowledgeURL)
	if err != nil {
		return err
	}
	defer knowledgeConn.Close()
	identity := peers.NewIdentity(identityConn, log)
	tokens := peers.NewTokens(identity, settings.GithubToken, settings.GithubAPIURL, config.DefaultGithubAPI, log)

	engineLog := logx.Engine(log)
	manager := clusters.NewManager(identity, netguard.New(settings.Net), settings.MaxClusters, log, engineLog)
	go manager.Janitor(hardStop)

	svc := gitops.New(gitops.Options{
		Store:         store.New(pool),
		Clusters:      manager,
		Repos:         github.New(tokens, github.DefaultLimits),
		Projects:      peers.NewKnowledge(knowledgeConn),
		Audit:         identity,
		Events:        events.NewHub(),
		Log:           log,
		EngineLog:     engineLog,
		Shutdown:      hardStop,
		SyncWaveDelay: 2 * time.Second,
	})
	api := server.New(svc, log)
	api.Leaving = leaving
	api.ForgetWorkspace = tokens.Forget

	reconciler := gitops.NewReconciler(svc, settings.Reconcile)
	go reconciler.Run(leaving)

	unary, streaming := server.Interceptors(log)
	grpcServer := grpc.NewServer(
		grpc.MaxRecvMsgSize(peers.MaxMessageBytes),
		grpc.MaxSendMsgSize(peers.MaxMessageBytes),
		grpc.ChainUnaryInterceptor(unary),
		grpc.ChainStreamInterceptor(streaming),
		grpc.KeepaliveEnforcementPolicy(keepalive.EnforcementPolicy{MinTime: 10 * time.Second, PermitWithoutStream: true}),
	)
	gitopsv1.RegisterGitopsServiceServer(grpcServer, api)
	healthServer := health.NewServer()
	healthpb.RegisterHealthServer(grpcServer, healthServer)
	healthServer.SetServingStatus(serviceName, healthpb.HealthCheckResponse_SERVING)
	healthServer.SetServingStatus("", healthpb.HealthCheckResponse_SERVING)

	listener, err := net.Listen("tcp", settings.Listen)
	if err != nil {
		return fmt.Errorf("could not listen on %s: %w", settings.Listen, err)
	}
	log.Info("gitops service listening",
		"addr", settings.Listen,
		"reconcile_seconds", int(settings.Reconcile/time.Second),
		"max_clusters", settings.MaxClusters,
		"private_deploy_targets", settings.Net.AllowPrivate,
		"github_token", settings.GithubToken != "",
	)
	served := make(chan error, 1)
	go func() { served <- grpcServer.Serve(listener) }()

	select {
	case err := <-served:
		return fmt.Errorf("gRPC server failed: %w", err)
	case <-leaving.Done():
	}
	log.Info("shutdown signal received")
	healthServer.Shutdown()
	stopped := make(chan struct{})
	go func() {
		grpcServer.GracefulStop()
		close(stopped)
	}()
	select {
	case <-stopped:
	case <-time.After(settings.ShutdownGrace):
		log.Warn("grace period over; terminating running operations")
		cancelHard()
		select {
		case <-stopped:
		case <-time.After(45 * time.Second):
			grpcServer.Stop()
		}
	}
	cancelHard()
	log.Info("gitops service stopped")
	return nil
}

// privateEngineTempDir points the engine's temporary files at a directory
// only this process can read. The engine's apply path (kubectl's libraries)
// needs a kubeconfig file and one manifest file per apply; they live there
// for the duration of one call, in memory (/dev/shm) where the system has
// it, and the directory goes when the process ends.
func privateEngineTempDir() (func(), error) {
	base := os.TempDir()
	if info, err := os.Stat("/dev/shm"); err == nil && info.IsDir() {
		base = "/dev/shm"
	}
	dir, err := os.MkdirTemp(base, "sdi-gitops-engine-")
	if err != nil {
		return nil, fmt.Errorf("could not create the engine's temporary directory: %w", err)
	}
	if err := os.Chmod(dir, 0o700); err != nil {
		return nil, fmt.Errorf("could not secure the engine's temporary directory: %w", err)
	}
	engineio.TempDir = filepath.Clean(dir)
	return func() { _ = os.RemoveAll(dir) }, nil
}

// healthcheck asks the local service's gRPC health endpoint.
func healthcheck() int {
	listen := os.Getenv("GITOPS_LISTEN")
	if listen == "" {
		listen = config.DefaultListen
	}
	_, port, err := net.SplitHostPort(listen)
	if err != nil {
		fmt.Fprintln(os.Stderr, "GITOPS_LISTEN is not host:port")
		return 1
	}
	conn, err := grpc.NewClient("127.0.0.1:"+port, grpc.WithTransportCredentials(insecure.NewCredentials()))
	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		return 1
	}
	defer conn.Close()
	ctx, cancel := context.WithTimeout(context.Background(), 3*time.Second)
	defer cancel()
	resp, err := healthpb.NewHealthClient(conn).Check(ctx, &healthpb.HealthCheckRequest{Service: serviceName})
	if err != nil || resp.GetStatus() != healthpb.HealthCheckResponse_SERVING {
		fmt.Fprintln(os.Stderr, "not serving")
		return 1
	}
	return 0
}

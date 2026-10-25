// Package config reads the service's settings from the environment
// (CONTRACT.md §3, §24). Everything comes from the environment so the same
// binary runs on a laptop and in a container without a config file.
package config

import (
	"fmt"
	"log/slog"
	"os"
	"strconv"
	"strings"
	"time"

	"github.com/f8fvwgzc/software-delivery-intelligence/services/gitops/internal/netguard"
)

const (
	DefaultListen          = "0.0.0.0:50055"
	DefaultDatabaseURL     = "postgres://sdi:sdi@127.0.0.1:5440/sdi"
	DefaultIdentityURL     = "http://127.0.0.1:50054"
	DefaultKnowledgeURL    = "http://127.0.0.1:50051"
	DefaultGithubAPI       = "https://api.github.com"
	DefaultReconcileSecs   = 180
	MinReconcileSecs       = 30
	DefaultMaxClusters     = 64
	DefaultShutdownGrace   = 20 * time.Second
	DefaultMaxConnections  = 20
	DefaultStatementMillis = 30_000
)

// Settings are the typed environment.
type Settings struct {
	Listen       string
	DatabaseURL  string
	IdentityURL  string
	KnowledgeURL string
	// Reconcile is the loop's period; 0 turns the loop off.
	Reconcile   time.Duration
	MaxClusters int
	Net         netguard.Policy
	// GithubToken and GithubAPIURL are the dev fallback used when identity
	// cannot be reached to mint a workspace token (as knowledge does, §18).
	GithubToken   string
	GithubAPIURL  string
	ShutdownGrace time.Duration
	Pool          PoolSettings
}

// PoolSettings mirror sdi_common::db::PoolSettings.
type PoolSettings struct {
	MaxConnections   int32
	MinConnections   int32
	AcquireTimeout   time.Duration
	IdleTimeout      time.Duration
	StatementTimeout time.Duration
}

// FromEnv reads and validates the settings.
func FromEnv() (Settings, error) {
	reconcile, err := envInt("GITOPS_RECONCILE_SECONDS", DefaultReconcileSecs)
	if err != nil {
		return Settings{}, err
	}
	if reconcile < 0 {
		return Settings{}, fmt.Errorf("GITOPS_RECONCILE_SECONDS must not be negative")
	}
	if reconcile > 0 && reconcile < MinReconcileSecs {
		slog.Warn("GITOPS_RECONCILE_SECONDS is below the minimum; using the minimum", "given", reconcile, "minimum", MinReconcileSecs)
		reconcile = MinReconcileSecs
	}
	maxClusters, err := envInt("GITOPS_MAX_CLUSTERS", DefaultMaxClusters)
	if err != nil {
		return Settings{}, err
	}
	if maxClusters < 1 {
		return Settings{}, fmt.Errorf("GITOPS_MAX_CLUSTERS must be at least 1")
	}
	grace, err := envInt("SHUTDOWN_GRACE_SECS", int(DefaultShutdownGrace/time.Second))
	if err != nil {
		return Settings{}, err
	}
	pool, err := poolFromEnv()
	if err != nil {
		return Settings{}, err
	}
	return Settings{
		Listen:        envOr("GITOPS_LISTEN", DefaultListen),
		DatabaseURL:   envOr("DATABASE_URL", DefaultDatabaseURL),
		IdentityURL:   envOr("IDENTITY_GRPC_URL", DefaultIdentityURL),
		KnowledgeURL:  envOr("KNOWLEDGE_GRPC_URL", DefaultKnowledgeURL),
		Reconcile:     time.Duration(reconcile) * time.Second,
		MaxClusters:   maxClusters,
		Net:           netguard.PolicyFromEnv(),
		GithubToken:   strings.TrimSpace(os.Getenv("GITHUB_TOKEN")),
		GithubAPIURL:  strings.TrimRight(envOr("GITHUB_API_URL", DefaultGithubAPI), "/"),
		ShutdownGrace: time.Duration(grace) * time.Second,
		Pool:          pool,
	}, nil
}

func poolFromEnv() (PoolSettings, error) {
	maxConns, err := envInt("DB_MAX_CONNECTIONS", DefaultMaxConnections)
	if err != nil {
		return PoolSettings{}, err
	}
	if maxConns < 1 {
		return PoolSettings{}, fmt.Errorf("DB_MAX_CONNECTIONS must be at least 1")
	}
	minConns, err := envInt("DB_MIN_CONNECTIONS", 0)
	if err != nil {
		return PoolSettings{}, err
	}
	acquire, err := envInt("DB_ACQUIRE_TIMEOUT_SECS", 10)
	if err != nil {
		return PoolSettings{}, err
	}
	idle, err := envInt("DB_IDLE_TIMEOUT_SECS", 600)
	if err != nil {
		return PoolSettings{}, err
	}
	statement, err := envInt("DB_STATEMENT_TIMEOUT_MS", DefaultStatementMillis)
	if err != nil {
		return PoolSettings{}, err
	}
	return PoolSettings{
		MaxConnections:   int32(maxConns),
		MinConnections:   int32(min(minConns, maxConns)),
		AcquireTimeout:   time.Duration(acquire) * time.Second,
		IdleTimeout:      time.Duration(idle) * time.Second,
		StatementTimeout: time.Duration(statement) * time.Millisecond,
	}, nil
}

// DefaultPool is the pool sizing without any environment (tests).
func DefaultPool() PoolSettings {
	return PoolSettings{
		MaxConnections:   DefaultMaxConnections,
		AcquireTimeout:   10 * time.Second,
		IdleTimeout:      600 * time.Second,
		StatementTimeout: DefaultStatementMillis * time.Millisecond,
	}
}

func envOr(key, fallback string) string {
	if v := strings.TrimSpace(os.Getenv(key)); v != "" {
		return v
	}
	return fallback
}

func envInt(key string, fallback int) (int, error) {
	raw := strings.TrimSpace(os.Getenv(key))
	if raw == "" {
		return fallback, nil
	}
	v, err := strconv.Atoi(raw)
	if err != nil {
		return 0, fmt.Errorf("%s has an invalid value: %s", key, raw)
	}
	return v, nil
}

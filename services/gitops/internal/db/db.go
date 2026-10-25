// Package db opens the service's Postgres pool and applies its migrations
// (CONTRACT.md §4). Every SDI service owns one schema; this one owns
// `gitops`, created when missing, and its pool's search_path is
// `gitops,public` so extension types in public still resolve.
package db

import (
	"context"
	"embed"
	"fmt"
	"io/fs"
	"regexp"
	"sort"
	"strconv"
	"strings"
	"time"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgxpool"

	"github.com/f8fvwgzc/software-delivery-intelligence/services/gitops/internal/config"
)

// Schema is the one this service owns.
const Schema = "gitops"

//go:embed migrations/*.sql
var migrationFiles embed.FS

var schemaName = regexp.MustCompile(`^[a-z_][a-z0-9_]{0,62}$`)

// Connect opens a pool on schema (created when missing) and migrates it.
func Connect(ctx context.Context, databaseURL, schema string, settings config.PoolSettings) (*pgxpool.Pool, error) {
	if !schemaName.MatchString(schema) {
		return nil, fmt.Errorf("schema name must be lowercase ascii: %s", schema)
	}
	base, err := pgx.ParseConfig(databaseURL)
	if err != nil {
		return nil, fmt.Errorf("DATABASE_URL is not valid")
	}
	connectCtx, cancel := context.WithTimeout(ctx, settings.AcquireTimeout)
	defer cancel()
	bootstrap, err := pgx.ConnectConfig(connectCtx, base)
	if err != nil {
		return nil, fmt.Errorf("could not reach Postgres: %w", err)
	}
	_, err = bootstrap.Exec(connectCtx, "CREATE SCHEMA IF NOT EXISTS "+schema)
	_ = bootstrap.Close(context.Background())
	if err != nil {
		return nil, fmt.Errorf("could not create schema %s: %w", schema, err)
	}

	cfg, err := pgxpool.ParseConfig(databaseURL)
	if err != nil {
		return nil, fmt.Errorf("DATABASE_URL is not valid")
	}
	cfg.ConnConfig.RuntimeParams["search_path"] = schema + ",public"
	if settings.StatementTimeout > 0 {
		cfg.ConnConfig.RuntimeParams["statement_timeout"] = strconv.FormatInt(settings.StatementTimeout.Milliseconds(), 10)
	}
	cfg.ConnConfig.RuntimeParams["application_name"] = "sdi-gitops"
	cfg.MaxConns = settings.MaxConnections
	cfg.MinConns = settings.MinConnections
	cfg.MaxConnIdleTime = settings.IdleTimeout
	cfg.HealthCheckPeriod = time.Minute
	pool, err := pgxpool.NewWithConfig(ctx, cfg)
	if err != nil {
		return nil, fmt.Errorf("could not open pool for schema %s: %w", schema, err)
	}
	pingCtx, cancelPing := context.WithTimeout(ctx, settings.AcquireTimeout)
	defer cancelPing()
	if err := pool.Ping(pingCtx); err != nil {
		pool.Close()
		return nil, fmt.Errorf("could not reach Postgres: %w", err)
	}
	if err := Migrate(ctx, pool); err != nil {
		pool.Close()
		return nil, err
	}
	return pool, nil
}

type migration struct {
	version int
	name    string
	sql     string
}

func loadMigrations() ([]migration, error) {
	entries, err := fs.ReadDir(migrationFiles, "migrations")
	if err != nil {
		return nil, err
	}
	var out []migration
	for _, entry := range entries {
		name := entry.Name()
		if !strings.HasSuffix(name, ".sql") {
			continue
		}
		prefix, _, ok := strings.Cut(name, "_")
		if !ok {
			return nil, fmt.Errorf("migration %s must be named <version>_<name>.sql", name)
		}
		version, err := strconv.Atoi(prefix)
		if err != nil {
			return nil, fmt.Errorf("migration %s must start with a number", name)
		}
		body, err := migrationFiles.ReadFile("migrations/" + name)
		if err != nil {
			return nil, err
		}
		out = append(out, migration{version: version, name: name, sql: string(body)})
	}
	sort.Slice(out, func(i, j int) bool { return out[i].version < out[j].version })
	for i := 1; i < len(out); i++ {
		if out[i].version == out[i-1].version {
			return nil, fmt.Errorf("two migrations share version %d", out[i].version)
		}
	}
	return out, nil
}

// Migrate applies the embedded migrations that have not run yet, in one
// transaction, serialised across replicas by an advisory lock so two
// instances starting together cannot both apply the same file.
func Migrate(ctx context.Context, pool *pgxpool.Pool) error {
	migrations, err := loadMigrations()
	if err != nil {
		return err
	}
	tx, err := pool.Begin(ctx)
	if err != nil {
		return fmt.Errorf("gitops migrations: %w", err)
	}
	defer func() { _ = tx.Rollback(context.Background()) }()
	if _, err := tx.Exec(ctx, `SELECT pg_advisory_xact_lock(hashtextextended('sdi.gitops.migrate:' || current_schema(), 0))`); err != nil {
		return fmt.Errorf("gitops migrations: %w", err)
	}
	if _, err := tx.Exec(ctx, `CREATE TABLE IF NOT EXISTS schema_migrations (
		version integer PRIMARY KEY,
		name text NOT NULL,
		applied_at timestamptz NOT NULL DEFAULT now()
	)`); err != nil {
		return fmt.Errorf("gitops migrations: %w", err)
	}
	applied := map[int]bool{}
	rows, err := tx.Query(ctx, `SELECT version FROM schema_migrations`)
	if err != nil {
		return fmt.Errorf("gitops migrations: %w", err)
	}
	for rows.Next() {
		var v int
		if err := rows.Scan(&v); err != nil {
			rows.Close()
			return fmt.Errorf("gitops migrations: %w", err)
		}
		applied[v] = true
	}
	rows.Close()
	if err := rows.Err(); err != nil {
		return fmt.Errorf("gitops migrations: %w", err)
	}
	for _, m := range migrations {
		if applied[m.version] {
			continue
		}
		if _, err := tx.Exec(ctx, m.sql); err != nil {
			return fmt.Errorf("gitops migration %s failed: %w", m.name, err)
		}
		if _, err := tx.Exec(ctx, `INSERT INTO schema_migrations (version, name) VALUES ($1, $2)`, m.version, m.name); err != nil {
			return fmt.Errorf("gitops migration %s failed: %w", m.name, err)
		}
	}
	if err := tx.Commit(ctx); err != nil {
		return fmt.Errorf("gitops migrations: %w", err)
	}
	return nil
}

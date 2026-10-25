package store

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"time"

	"github.com/google/uuid"
	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgconn"
	"github.com/jackc/pgx/v5/pgxpool"
)

var (
	// ErrNotFound is a row that does not exist in the caller's workspace.
	ErrNotFound = errors.New("not found")
	// ErrConflict is a unique key taken by another row.
	ErrConflict = errors.New("conflict")
	// ErrInUse is a cluster that applications still target.
	ErrInUse = errors.New("in use")
)

// MaxHistory is how many history entries are kept per application.
const MaxHistory = 50

// Store is the service's data access.
type Store struct {
	pool *pgxpool.Pool
}

// New wraps a pool whose search_path is the gitops schema.
func New(pool *pgxpool.Pool) *Store {
	return &Store{pool: pool}
}

// Pool is the underlying pool (the reconcile loop's leadership lock).
func (s *Store) Pool() *pgxpool.Pool {
	return s.pool
}

func mapErr(err error) error {
	if errors.Is(err, pgx.ErrNoRows) {
		return ErrNotFound
	}
	var pgErr *pgconn.PgError
	if errors.As(err, &pgErr) {
		switch pgErr.Code {
		case "23505":
			return ErrConflict
		case "23503":
			return ErrInUse
		}
	}
	return err
}

// --- Clusters ---------------------------------------------------------------

const clusterColumns = `c.id, c.workspace_id, c.name, c.kubeconfig_secret, c.context, c.server,
	c.default_namespace, c.namespaces, c.status, c.message, c.server_version, c.checked_at,
	c.created_at, c.updated_at`

func scanCluster(row pgx.Row, extra ...any) (Cluster, error) {
	var c Cluster
	dest := []any{&c.ID, &c.WorkspaceID, &c.Name, &c.KubeconfigSecret, &c.Context, &c.Server,
		&c.DefaultNamespace, &c.Namespaces, &c.Status, &c.Message, &c.ServerVersion, &c.CheckedAt,
		&c.CreatedAt, &c.UpdatedAt}
	dest = append(dest, extra...)
	if err := row.Scan(dest...); err != nil {
		return Cluster{}, mapErr(err)
	}
	if c.Namespaces == nil {
		c.Namespaces = []string{}
	}
	return c, nil
}

// ListClusters returns the workspace's clusters with how many applications
// target each.
func (s *Store) ListClusters(ctx context.Context, workspace uuid.UUID) ([]Cluster, error) {
	rows, err := s.pool.Query(ctx, `SELECT `+clusterColumns+`,
		(SELECT count(*) FROM applications a WHERE a.workspace_id = c.workspace_id AND a.cluster_id = c.id)
		FROM clusters c WHERE c.workspace_id = $1 ORDER BY c.name`, workspace)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	var out []Cluster
	for rows.Next() {
		var count int
		c, err := scanCluster(rows, &count)
		if err != nil {
			return nil, err
		}
		c.ApplicationCount = count
		out = append(out, c)
	}
	return out, rows.Err()
}

// GetCluster is one cluster of the workspace.
func (s *Store) GetCluster(ctx context.Context, workspace, id uuid.UUID) (Cluster, error) {
	var count int
	c, err := scanCluster(s.pool.QueryRow(ctx, `SELECT `+clusterColumns+`,
		(SELECT count(*) FROM applications a WHERE a.workspace_id = c.workspace_id AND a.cluster_id = c.id)
		FROM clusters c WHERE c.workspace_id = $1 AND c.id = $2`, workspace, id), &count)
	c.ApplicationCount = count
	return c, err
}

// UpsertCluster inserts the cluster or updates it by id, in its workspace.
func (s *Store) UpsertCluster(ctx context.Context, c Cluster) (Cluster, error) {
	namespaces := c.Namespaces
	if namespaces == nil {
		namespaces = []string{}
	}
	row := s.pool.QueryRow(ctx, `INSERT INTO clusters AS c (id, workspace_id, name, kubeconfig_secret, context,
			server, default_namespace, namespaces, status, message, server_version, checked_at)
		VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
		ON CONFLICT (id) DO UPDATE SET name = EXCLUDED.name, kubeconfig_secret = EXCLUDED.kubeconfig_secret,
			context = EXCLUDED.context, server = EXCLUDED.server, default_namespace = EXCLUDED.default_namespace,
			namespaces = EXCLUDED.namespaces, status = EXCLUDED.status, message = EXCLUDED.message,
			server_version = EXCLUDED.server_version, checked_at = EXCLUDED.checked_at, updated_at = clock_timestamp()
		WHERE c.workspace_id = EXCLUDED.workspace_id
		RETURNING `+clusterColumns,
		c.ID, c.WorkspaceID, c.Name, c.KubeconfigSecret, c.Context, c.Server, c.DefaultNamespace,
		namespaces, c.Status, c.Message, c.ServerVersion, c.CheckedAt)
	return scanCluster(row)
}

// CountClusters is how many clusters a workspace has.
func (s *Store) CountClusters(ctx context.Context, workspace uuid.UUID) (int, error) {
	var n int
	err := s.pool.QueryRow(ctx, `SELECT count(*) FROM clusters WHERE workspace_id = $1`, workspace).Scan(&n)
	return n, err
}

// CountApplications is how many applications a workspace has.
func (s *Store) CountApplications(ctx context.Context, workspace uuid.UUID) (int, error) {
	var n int
	err := s.pool.QueryRow(ctx, `SELECT count(*) FROM applications WHERE workspace_id = $1`, workspace).Scan(&n)
	return n, err
}

// DeleteCluster removes a cluster no application targets.
func (s *Store) DeleteCluster(ctx context.Context, workspace, id uuid.UUID) error {
	tag, err := s.pool.Exec(ctx, `DELETE FROM clusters WHERE workspace_id = $1 AND id = $2`, workspace, id)
	if err != nil {
		return mapErr(err)
	}
	if tag.RowsAffected() == 0 {
		return ErrNotFound
	}
	return nil
}

// --- Applications -------------------------------------------------------------

const appColumns = `a.id, a.workspace_id, a.project_id, a.name, a.cluster_id, c.name, a.namespace,
	a.source, a.sync_policy, a.sync_status, a.health_status, a.health_message, a.revision,
	a.desired_digest, a.synced_revision, a.synced_digest, a.conditions, a.resources,
	a.resource_count, a.out_of_sync_count, a.reconciled_at, a.operation, a.op_lease_until,
	a.auto_digest, a.auto_attempts, a.auto_next_at, a.created_at, a.updated_at`

const appFrom = ` FROM applications a JOIN clusters c ON c.workspace_id = a.workspace_id AND c.id = a.cluster_id `

func scanApp(row pgx.Row, withResources bool) (Application, error) {
	var a Application
	var source, policy, conditions, resources, operation []byte
	if err := row.Scan(&a.ID, &a.WorkspaceID, &a.ProjectID, &a.Name, &a.ClusterID, &a.ClusterName,
		&a.Namespace, &source, &policy, &a.SyncStatus, &a.HealthStatus, &a.HealthMessage, &a.Revision,
		&a.DesiredDigest, &a.SyncedRevision, &a.SyncedDigest, &conditions, &resources,
		&a.ResourceCount, &a.OutOfSyncCount, &a.ReconciledAt, &operation, &a.OpLeaseUntil,
		&a.AutoDigest, &a.AutoAttempts, &a.AutoNextAt, &a.CreatedAt, &a.UpdatedAt); err != nil {
		return Application{}, mapErr(err)
	}
	if err := json.Unmarshal(source, &a.Source); err != nil {
		return Application{}, fmt.Errorf("application %s: source: %w", a.ID, err)
	}
	if err := json.Unmarshal(policy, &a.Policy); err != nil {
		return Application{}, fmt.Errorf("application %s: sync policy: %w", a.ID, err)
	}
	if err := json.Unmarshal(conditions, &a.Conditions); err != nil {
		return Application{}, fmt.Errorf("application %s: conditions: %w", a.ID, err)
	}
	if withResources {
		if err := json.Unmarshal(resources, &a.Resources); err != nil {
			return Application{}, fmt.Errorf("application %s: resources: %w", a.ID, err)
		}
	}
	if len(operation) > 0 && string(operation) != "null" {
		var op Operation
		if err := json.Unmarshal(operation, &op); err != nil {
			return Application{}, fmt.Errorf("application %s: operation: %w", a.ID, err)
		}
		a.Operation = &op
	}
	return a, nil
}

// ListApplications returns the workspace's applications (of one project
// when projectID is set), without their resource trees.
func (s *Store) ListApplications(ctx context.Context, workspace uuid.UUID, projectID string) ([]Application, error) {
	rows, err := s.pool.Query(ctx, `SELECT `+appColumns+appFrom+`
		WHERE a.workspace_id = $1 AND ($2 = '' OR a.project_id = $2) ORDER BY a.name, a.id`, workspace, projectID)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	var out []Application
	for rows.Next() {
		a, err := scanApp(rows, false)
		if err != nil {
			return nil, err
		}
		out = append(out, a)
	}
	return out, rows.Err()
}

// GetApplication is one application of the workspace with its resources.
func (s *Store) GetApplication(ctx context.Context, workspace, id uuid.UUID) (Application, error) {
	return scanApp(s.pool.QueryRow(ctx, `SELECT `+appColumns+appFrom+`WHERE a.workspace_id = $1 AND a.id = $2`, workspace, id), true)
}

// InsertApplication creates an application.
func (s *Store) InsertApplication(ctx context.Context, a Application) error {
	source, policy, err := specJSON(a)
	if err != nil {
		return err
	}
	_, err = s.pool.Exec(ctx, `INSERT INTO applications (id, workspace_id, project_id, name, cluster_id,
			namespace, source, sync_policy) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)`,
		a.ID, a.WorkspaceID, a.ProjectID, a.Name, a.ClusterID, a.Namespace, source, policy)
	return mapErr(err)
}

// UpdateApplicationSpec changes what an admin declares (name, destination,
// source, policy) and keeps everything the comparisons found. A changed
// destination resets the statuses, since they describe another place.
func (s *Store) UpdateApplicationSpec(ctx context.Context, a Application) error {
	source, policy, err := specJSON(a)
	if err != nil {
		return err
	}
	tag, err := s.pool.Exec(ctx, `UPDATE applications SET
			name = $3, cluster_id = $4, namespace = $5, source = $6, sync_policy = $7,
			sync_status = CASE WHEN cluster_id <> $4 OR namespace <> $5 THEN 'unknown' ELSE sync_status END,
			health_status = CASE WHEN cluster_id <> $4 OR namespace <> $5 THEN 'unknown' ELSE health_status END,
			resources = CASE WHEN cluster_id <> $4 OR namespace <> $5 THEN '[]'::jsonb ELSE resources END,
			auto_digest = '', auto_attempts = 0, auto_next_at = NULL,
			updated_at = clock_timestamp()
		WHERE workspace_id = $1 AND id = $2`,
		a.WorkspaceID, a.ID, a.Name, a.ClusterID, a.Namespace, source, policy)
	if err != nil {
		return mapErr(err)
	}
	if tag.RowsAffected() == 0 {
		return ErrNotFound
	}
	return nil
}

func specJSON(a Application) ([]byte, []byte, error) {
	source, err := json.Marshal(a.Source)
	if err != nil {
		return nil, nil, err
	}
	policy, err := json.Marshal(a.Policy)
	if err != nil {
		return nil, nil, err
	}
	return source, policy, nil
}

// UpdateSource stores a new source (a sync's image overrides and helm
// parameters), keeping the desired state declared.
func (s *Store) UpdateSource(ctx context.Context, workspace, id uuid.UUID, source Source) error {
	raw, err := json.Marshal(source)
	if err != nil {
		return err
	}
	tag, err := s.pool.Exec(ctx, `UPDATE applications SET source = $3, updated_at = clock_timestamp()
		WHERE workspace_id = $1 AND id = $2`, workspace, id, raw)
	if err != nil {
		return mapErr(err)
	}
	if tag.RowsAffected() == 0 {
		return ErrNotFound
	}
	return nil
}

// DeleteApplication removes an application and its history.
func (s *Store) DeleteApplication(ctx context.Context, workspace, id uuid.UUID) error {
	tag, err := s.pool.Exec(ctx, `DELETE FROM applications WHERE workspace_id = $1 AND id = $2`, workspace, id)
	if err != nil {
		return mapErr(err)
	}
	if tag.RowsAffected() == 0 {
		return ErrNotFound
	}
	return nil
}

// SaveStatus records a comparison that completed.
func (s *Store) SaveStatus(ctx context.Context, workspace, id uuid.UUID, st Status) error {
	conditions, err := json.Marshal(nonNil(st.Conditions))
	if err != nil {
		return err
	}
	resources := st.Resources
	if resources == nil {
		resources = []ResourceStatus{}
	}
	rawResources, err := json.Marshal(resources)
	if err != nil {
		return err
	}
	_, err = s.pool.Exec(ctx, `UPDATE applications SET sync_status = $3, health_status = $4,
			health_message = $5, revision = $6, desired_digest = $7, conditions = $8, resources = $9,
			resource_count = $10, out_of_sync_count = $11, reconciled_at = $12
		WHERE workspace_id = $1 AND id = $2`,
		workspace, id, st.SyncStatus, st.HealthStatus, st.HealthMessage, st.Revision, st.DesiredDigest,
		conditions, rawResources, st.ResourceCount, st.OutOfSyncCount, st.ReconciledAt)
	return mapErr(err)
}

// SaveConditions records a comparison that could not finish: the last
// known statuses stay, the reasons replace the conditions.
func (s *Store) SaveConditions(ctx context.Context, workspace, id uuid.UUID, conditions []string) error {
	raw, err := json.Marshal(nonNil(conditions))
	if err != nil {
		return err
	}
	_, err = s.pool.Exec(ctx, `UPDATE applications SET conditions = $3 WHERE workspace_id = $1 AND id = $2`, workspace, id, raw)
	return mapErr(err)
}

func nonNil(v []string) []string {
	if v == nil {
		return []string{}
	}
	return v
}

// ClaimOperation starts an operation when none holds the application: the
// lease is the cross-replica "one operation at a time" (§24.5). False when
// another operation's lease has not expired.
func (s *Store) ClaimOperation(ctx context.Context, workspace, id uuid.UUID, op Operation, lease time.Duration) (bool, error) {
	raw, err := json.Marshal(op)
	if err != nil {
		return false, err
	}
	tag, err := s.pool.Exec(ctx, `UPDATE applications SET operation = $3,
			op_lease_until = clock_timestamp() + make_interval(secs => $4)
		WHERE workspace_id = $1 AND id = $2 AND (op_lease_until IS NULL OR op_lease_until < clock_timestamp())`,
		workspace, id, raw, lease.Seconds())
	if err != nil {
		return false, mapErr(err)
	}
	return tag.RowsAffected() == 1, nil
}

// RenewLease keeps a long operation's claim alive.
func (s *Store) RenewLease(ctx context.Context, workspace, id uuid.UUID, lease time.Duration) error {
	_, err := s.pool.Exec(ctx, `UPDATE applications SET op_lease_until = clock_timestamp() + make_interval(secs => $3)
		WHERE workspace_id = $1 AND id = $2 AND op_lease_until IS NOT NULL`, workspace, id, lease.Seconds())
	return mapErr(err)
}

// FinishOperation records the operation's outcome (nil keeps none) and
// releases the lease. synced, when set, is the revision and digest a
// successful sync applied.
func (s *Store) FinishOperation(ctx context.Context, workspace, id uuid.UUID, op *Operation, synced *[2]string) error {
	var raw []byte
	if op != nil {
		var err error
		if raw, err = json.Marshal(op); err != nil {
			return err
		}
	}
	var err error
	if synced != nil {
		_, err = s.pool.Exec(ctx, `UPDATE applications SET operation = $3, op_lease_until = NULL,
				synced_revision = $4, synced_digest = $5 WHERE workspace_id = $1 AND id = $2`,
			workspace, id, raw, synced[0], synced[1])
	} else {
		_, err = s.pool.Exec(ctx, `UPDATE applications SET operation = $3, op_lease_until = NULL
			WHERE workspace_id = $1 AND id = $2`, workspace, id, raw)
	}
	return mapErr(err)
}

// SaveAutomation records automated sync bookkeeping.
func (s *Store) SaveAutomation(ctx context.Context, workspace, id uuid.UUID, digest string, attempts int, next *time.Time) error {
	_, err := s.pool.Exec(ctx, `UPDATE applications SET auto_digest = $3, auto_attempts = $4, auto_next_at = $5
		WHERE workspace_id = $1 AND id = $2`, workspace, id, digest, attempts, next)
	return mapErr(err)
}

// AllApplications addresses every application of every workspace, for the
// reconcile loop only (it then works inside each one's workspace).
func (s *Store) AllApplications(ctx context.Context) ([]AppRef, error) {
	rows, err := s.pool.Query(ctx, `SELECT id, workspace_id FROM applications ORDER BY reconciled_at NULLS FIRST, id`)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	var out []AppRef
	for rows.Next() {
		var r AppRef
		if err := rows.Scan(&r.ID, &r.WorkspaceID); err != nil {
			return nil, err
		}
		out = append(out, r)
	}
	return out, rows.Err()
}

// --- History --------------------------------------------------------------------

// InsertHistory appends an entry and keeps the newest MaxHistory.
func (s *Store) InsertHistory(ctx context.Context, h HistoryEntry) error {
	source, err := json.Marshal(h.Source)
	if err != nil {
		return err
	}
	tx, err := s.pool.Begin(ctx)
	if err != nil {
		return err
	}
	defer func() { _ = tx.Rollback(context.Background()) }()
	if _, err := tx.Exec(ctx, `INSERT INTO history (id, workspace_id, application_id, kind, revision, source,
			phase, message, initiated_by, execution_id, automated, started_at, finished_at)
		VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)`,
		h.ID, h.WorkspaceID, h.ApplicationID, h.Kind, h.Revision, source, h.Phase, h.Message,
		h.InitiatedBy, h.ExecutionID, h.Automated, h.StartedAt, h.FinishedAt); err != nil {
		return mapErr(err)
	}
	if _, err := tx.Exec(ctx, `DELETE FROM history WHERE workspace_id = $1 AND application_id = $2 AND id NOT IN (
			SELECT id FROM history WHERE workspace_id = $1 AND application_id = $2
			ORDER BY started_at DESC, id DESC LIMIT $3)`, h.WorkspaceID, h.ApplicationID, MaxHistory); err != nil {
		return err
	}
	return tx.Commit(ctx)
}

const historyColumns = `id, workspace_id, application_id, kind, revision, source, phase, message,
	initiated_by, execution_id, automated, started_at, finished_at`

func scanHistory(row pgx.Row) (HistoryEntry, error) {
	var h HistoryEntry
	var source []byte
	if err := row.Scan(&h.ID, &h.WorkspaceID, &h.ApplicationID, &h.Kind, &h.Revision, &source, &h.Phase,
		&h.Message, &h.InitiatedBy, &h.ExecutionID, &h.Automated, &h.StartedAt, &h.FinishedAt); err != nil {
		return HistoryEntry{}, mapErr(err)
	}
	if err := json.Unmarshal(source, &h.Source); err != nil {
		return HistoryEntry{}, fmt.Errorf("history %s: source: %w", h.ID, err)
	}
	return h, nil
}

// ListHistory is an application's history, newest first.
func (s *Store) ListHistory(ctx context.Context, workspace, appID uuid.UUID, limit int) ([]HistoryEntry, error) {
	rows, err := s.pool.Query(ctx, `SELECT `+historyColumns+` FROM history
		WHERE workspace_id = $1 AND application_id = $2 ORDER BY started_at DESC, id DESC LIMIT $3`,
		workspace, appID, limit)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	var out []HistoryEntry
	for rows.Next() {
		h, err := scanHistory(rows)
		if err != nil {
			return nil, err
		}
		out = append(out, h)
	}
	return out, rows.Err()
}

// GetHistory is one entry of one application.
func (s *Store) GetHistory(ctx context.Context, workspace, appID, id uuid.UUID) (HistoryEntry, error) {
	return scanHistory(s.pool.QueryRow(ctx, `SELECT `+historyColumns+` FROM history
		WHERE workspace_id = $1 AND application_id = $2 AND id = $3`, workspace, appID, id))
}

// --- Workspace data (§19) --------------------------------------------------------

// Tables are the workspace's tables, parents before children, with the
// entity name each export record carries and the export order.
var Tables = []struct{ Entity, Table, Order string }{
	{"cluster", "clusters", "name, id"},
	{"application", "applications", "project_id, name, id"},
	{"history", "history", "application_id, started_at, id"},
}

// ExportWorkspace hands every row of the workspace, encoded by Postgres
// (row_to_json), to emit, table by table, read through a cursor.
func (s *Store) ExportWorkspace(ctx context.Context, workspace uuid.UUID, emit func(entity, json string) error) error {
	for _, t := range Tables {
		// Table and order come from Tables, never from a caller.
		rows, err := s.pool.Query(ctx, `SELECT row_to_json(t)::text FROM (SELECT * FROM `+t.Table+
			` WHERE workspace_id = $1 ORDER BY `+t.Order+`) t`, workspace)
		if err != nil {
			return err
		}
		for rows.Next() {
			var line string
			if err := rows.Scan(&line); err != nil {
				rows.Close()
				return err
			}
			if err := emit(t.Entity, line); err != nil {
				rows.Close()
				return err
			}
		}
		rows.Close()
		if err := rows.Err(); err != nil {
			return err
		}
	}
	return nil
}

// DeleteWorkspace deletes every row of the workspace in one transaction,
// children first so the counts are honest. Returns rows per entity.
func (s *Store) DeleteWorkspace(ctx context.Context, workspace uuid.UUID) (map[string]uint64, error) {
	tx, err := s.pool.Begin(ctx)
	if err != nil {
		return nil, err
	}
	defer func() { _ = tx.Rollback(context.Background()) }()
	deleted := map[string]uint64{}
	for i := len(Tables) - 1; i >= 0; i-- {
		t := Tables[i]
		tag, err := tx.Exec(ctx, `DELETE FROM `+t.Table+` WHERE workspace_id = $1`, workspace)
		if err != nil {
			return nil, err
		}
		deleted[t.Entity] = uint64(tag.RowsAffected())
	}
	if err := tx.Commit(ctx); err != nil {
		return nil, err
	}
	return deleted, nil
}

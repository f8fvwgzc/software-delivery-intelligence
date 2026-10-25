// Package peers are the gRPC clients of the services gitops calls:
// identity (workspace secrets, GitHub tokens, audit) and knowledge (the
// project an application belongs to). Every call carries a principal's
// metadata; internal work uses the workspace's system principal, which is
// what identity's ResolveSecret requires (OWNER, the caller's workspace).
package peers

import (
	"context"
	"fmt"
	"log/slog"
	"strings"
	"sync"
	"time"

	"github.com/google/uuid"
	"google.golang.org/grpc"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/credentials/insecure"
	"google.golang.org/grpc/keepalive"
	"google.golang.org/grpc/status"
	"google.golang.org/protobuf/types/known/timestamppb"

	identityv1 "github.com/f8fvwgzc/software-delivery-intelligence/services/gitops/gen/sdi/identity/v1"
	knowledgev1 "github.com/f8fvwgzc/software-delivery-intelligence/services/gitops/gen/sdi/knowledge/v1"
	"github.com/f8fvwgzc/software-delivery-intelligence/services/gitops/internal/github"
	"github.com/f8fvwgzc/software-delivery-intelligence/services/gitops/internal/principal"
)

// MaxMessageBytes is the largest gRPC message sent or received (16 MiB),
// as sdi_common::grpc::MAX_MESSAGE_BYTES.
const MaxMessageBytes = 16 << 20

const callTimeout = 5 * time.Second

// Dial is a lazily connected channel: the service starts even when its
// peers are not up yet, and reconnects on its own once they are.
func Dial(raw string) (*grpc.ClientConn, error) {
	target := strings.TrimPrefix(strings.TrimPrefix(raw, "http://"), "https://")
	if target == "" {
		return nil, fmt.Errorf("invalid gRPC endpoint: %s", raw)
	}
	return grpc.NewClient(target,
		grpc.WithTransportCredentials(insecure.NewCredentials()),
		grpc.WithDefaultCallOptions(grpc.MaxCallRecvMsgSize(MaxMessageBytes), grpc.MaxCallSendMsgSize(MaxMessageBytes)),
		grpc.WithKeepaliveParams(keepalive.ClientParameters{Time: 30 * time.Second, Timeout: 10 * time.Second, PermitWithoutStream: true}),
	)
}

// Unreachable is identity's (or any peer's) failure to answer at all, as
// opposed to an answer that refuses.
func Unreachable(err error) bool {
	switch status.Code(err) {
	case codes.Unavailable, codes.DeadlineExceeded, codes.Unimplemented:
		return true
	}
	return false
}

// --- Identity ---------------------------------------------------------------------

// Identity is the identity service.
type Identity struct {
	client identityv1.IdentityServiceClient
	log    *slog.Logger
	audits chan struct{}
}

// NewIdentity wraps a connection to identity.
func NewIdentity(conn grpc.ClientConnInterface, log *slog.Logger) *Identity {
	return &Identity{client: identityv1.NewIdentityServiceClient(conn), log: log, audits: make(chan struct{}, 32)}
}

// ResolveSecret reads a workspace secret's value, as the workspace's system
// principal; the value lives in memory for the caller only.
func (i *Identity) ResolveSecret(ctx context.Context, workspace uuid.UUID, name string) (string, error) {
	ctx, cancel := context.WithTimeout(principal.System(workspace).Outgoing(ctx), callTimeout)
	defer cancel()
	resp, err := i.client.ResolveSecret(ctx, &identityv1.ResolveSecretRequest{WorkspaceId: workspace.String(), Name: name})
	if err != nil {
		return "", err
	}
	return resp.GetValue(), nil
}

// RecordAudit writes an audit entry in the background: audit must never
// slow down or fail the action it records. When too many writes are in
// flight the entry is dropped with a warning.
func (i *Identity) RecordAudit(p principal.Principal, action, entity, entityID, detail string) {
	entry := &identityv1.AuditEntry{
		WorkspaceId: p.WorkspaceID.String(),
		UserId:      p.UserID,
		Action:      action,
		Entity:      entity,
		EntityId:    entityID,
		Detail:      detail,
		At:          timestamppb.Now(),
	}
	select {
	case i.audits <- struct{}{}:
	default:
		i.log.Warn("audit backlog full; entry dropped", "action", action, "entity", entity, "entity_id", entityID)
		return
	}
	go func() {
		defer func() { <-i.audits }()
		ctx, cancel := context.WithTimeout(p.Outgoing(context.Background()), callTimeout)
		defer cancel()
		if _, err := i.client.RecordAudit(ctx, &identityv1.RecordAuditRequest{Entry: entry}); err != nil {
			i.log.Warn("could not record audit entry", "action", action, "error", status.Convert(err).Message())
		}
	}()
}

// installationToken mints the workspace's GitHub token.
func (i *Identity) installationToken(ctx context.Context, workspace uuid.UUID) (*identityv1.InstallationToken, error) {
	ctx, cancel := context.WithTimeout(principal.System(workspace).Outgoing(ctx), callTimeout)
	defer cancel()
	return i.client.GetInstallationToken(ctx, &identityv1.GetInstallationTokenRequest{WorkspaceId: workspace.String()})
}

// --- GitHub tokens --------------------------------------------------------------------

const (
	tokenCacheCapacity = 1024
	refreshMargin      = 5 * time.Minute
	defaultLifetime    = 55 * time.Minute
	devRetry           = 5 * time.Minute
)

type cachedAccess struct {
	access  github.Access
	expires time.Time
	used    time.Time
}

// Tokens is the workspace token source of CONTRACT.md §18: identity mints
// the token (installation token or the owner's OAuth token, with the
// host's API base), cached in memory until five minutes before it
// expires. When identity cannot be reached and GITHUB_TOKEN is set, that
// token with GITHUB_API_URL is used for every workspace (dev mode, warned
// once), exactly as knowledge falls back.
type Tokens struct {
	identity  *Identity
	devToken  string
	devBase   string
	log       *slog.Logger
	mu        sync.Mutex
	cache     map[uuid.UUID]cachedAccess
	warnOnce  sync.Once
	publicAPI string
}

// NewTokens builds the token source.
func NewTokens(identity *Identity, devToken, devBase, publicAPI string, log *slog.Logger) *Tokens {
	return &Tokens{identity: identity, devToken: devToken, devBase: strings.TrimRight(devBase, "/"),
		publicAPI: strings.TrimRight(publicAPI, "/"), log: log, cache: map[uuid.UUID]cachedAccess{}}
}

// Access implements github.TokenSource.
func (t *Tokens) Access(ctx context.Context, workspace uuid.UUID) (github.Access, error) {
	now := time.Now()
	t.mu.Lock()
	if c, ok := t.cache[workspace]; ok && c.expires.Add(-refreshMargin).After(now) {
		c.used = now
		t.cache[workspace] = c
		t.mu.Unlock()
		return c.access, nil
	}
	t.mu.Unlock()

	token, err := t.identity.installationToken(ctx, workspace)
	var access github.Access
	expires := now.Add(defaultLifetime)
	switch {
	case err == nil:
		if strings.TrimSpace(token.GetToken()) == "" {
			return github.Access{}, &github.Error{Message: "identity returned no GitHub token for this workspace"}
		}
		base := strings.TrimRight(strings.TrimSpace(token.GetApiBaseUrl()), "/")
		if base == "" {
			base = t.publicAPI
		}
		access = github.Access{Token: token.GetToken(), APIBase: base}
		if token.GetExpiresAt() != nil {
			expires = token.GetExpiresAt().AsTime()
		}
	case Unreachable(err) && t.devToken != "":
		t.warnOnce.Do(func() {
			t.log.Warn("identity service unreachable; using GITHUB_TOKEN for every workspace (dev mode)", "code", status.Code(err).String())
		})
		access = github.Access{Token: t.devToken, APIBase: t.devBase}
		expires = now.Add(devRetry + refreshMargin)
	case Unreachable(err):
		return github.Access{}, &github.Error{Message: "identity service unreachable (" + status.Convert(err).Message() + ") and GITHUB_TOKEN is not set"}
	default:
		return github.Access{}, &github.Error{Message: "no GitHub access for this workspace: " + status.Convert(err).Message()}
	}
	t.mu.Lock()
	defer t.mu.Unlock()
	if len(t.cache) >= tokenCacheCapacity {
		t.evictOldest()
	}
	t.cache[workspace] = cachedAccess{access: access, expires: expires, used: now}
	return access, nil
}

func (t *Tokens) evictOldest() {
	var oldest uuid.UUID
	var at time.Time
	first := true
	for id, c := range t.cache {
		if first || c.used.Before(at) {
			oldest, at, first = id, c.used, false
		}
	}
	delete(t.cache, oldest)
}

// Forget drops a workspace's cached token (GitHub rejected it, or the
// workspace's data was deleted).
func (t *Tokens) Forget(workspace uuid.UUID) {
	t.mu.Lock()
	delete(t.cache, workspace)
	t.mu.Unlock()
}

// --- Knowledge ----------------------------------------------------------------------------

// Project is what gitops needs of a knowledge project.
type Project struct {
	ID            string
	RepoFullName  string
	DefaultBranch string
}

// Knowledge is the knowledge service.
type Knowledge struct {
	client knowledgev1.KnowledgeServiceClient
}

// NewKnowledge wraps a connection to knowledge.
func NewKnowledge(conn grpc.ClientConnInterface) *Knowledge {
	return &Knowledge{client: knowledgev1.NewKnowledgeServiceClient(conn)}
}

// GetProject reads a project of p's workspace (NOT_FOUND for another
// workspace's).
func (k *Knowledge) GetProject(ctx context.Context, p principal.Principal, id string) (Project, error) {
	ctx, cancel := context.WithTimeout(p.Outgoing(ctx), callTimeout)
	defer cancel()
	project, err := k.client.GetProject(ctx, &knowledgev1.GetProjectRequest{Id: id})
	if err != nil {
		return Project{}, err
	}
	return Project{ID: project.GetId(), RepoFullName: project.GetRepoFullName(), DefaultBranch: project.GetDefaultBranch()}, nil
}

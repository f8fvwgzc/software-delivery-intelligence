// Package principal is the caller's identity on every gRPC call
// (CONTRACT.md §18), mirroring crates/common/src/principal.rs.
//
// The gateway authenticates once and forwards x-sdi-user, x-sdi-workspace
// and x-sdi-role as metadata. The service never trusts a workspace id from a
// request body: it reads it from here and passes it to every store function.
package principal

import (
	"context"
	"strings"

	"github.com/google/uuid"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/metadata"
	"google.golang.org/grpc/status"
)

const (
	UserHeader      = "x-sdi-user"
	WorkspaceHeader = "x-sdi-workspace"
	RoleHeader      = "x-sdi-role"

	// SystemUser is who internal work (the reconcile loop, automated syncs)
	// runs as.
	SystemUser = "system"
)

// Role is ordered: each role includes the ones below it.
type Role int

const (
	Viewer Role = iota + 1
	Member
	Approver
	Admin
	Owner
)

// ParseRole reads the lowercase role name the gateway forwards.
func ParseRole(s string) (Role, bool) {
	switch s {
	case "viewer":
		return Viewer, true
	case "member":
		return Member, true
	case "approver":
		return Approver, true
	case "admin":
		return Admin, true
	case "owner":
		return Owner, true
	}
	return 0, false
}

func (r Role) String() string {
	switch r {
	case Viewer:
		return "viewer"
	case Member:
		return "member"
	case Approver:
		return "approver"
	case Admin:
		return "admin"
	case Owner:
		return "owner"
	}
	return "unknown"
}

// Principal is who calls and in which workspace.
type Principal struct {
	UserID      string
	WorkspaceID uuid.UUID
	Role        Role
}

// FromIncoming reads the three headers of an incoming call. Missing or
// malformed metadata is UNAUTHENTICATED, because a call without a workspace
// cannot be scoped; the workspace must be a uuid, as in knowledge (§18).
func FromIncoming(ctx context.Context) (Principal, error) {
	md, _ := metadata.FromIncomingContext(ctx)
	get := func(key string) (string, error) {
		values := md.Get(key)
		if len(values) == 0 || strings.TrimSpace(values[0]) == "" {
			return "", status.Errorf(codes.Unauthenticated, "missing %s", key)
		}
		return strings.TrimSpace(values[0]), nil
	}
	rawRole, err := get(RoleHeader)
	if err != nil {
		return Principal{}, err
	}
	user, err := get(UserHeader)
	if err != nil {
		return Principal{}, err
	}
	rawWorkspace, err := get(WorkspaceHeader)
	if err != nil {
		return Principal{}, err
	}
	role, ok := ParseRole(rawRole)
	if !ok {
		return Principal{}, status.Error(codes.Unauthenticated, "bad role")
	}
	workspace, err := uuid.Parse(rawWorkspace)
	if err != nil {
		return Principal{}, status.Error(codes.Unauthenticated, "workspace id is not a valid id")
	}
	return Principal{UserID: user, WorkspaceID: workspace, Role: role}, nil
}

// Require is PERMISSION_DENIED unless the caller holds at least min.
func (p Principal) Require(min Role) error {
	if p.Role >= min {
		return nil
	}
	return status.Errorf(codes.PermissionDenied, "requires the %s role", min)
}

// IsSystem tells internal work apart from a person.
func (p Principal) IsSystem() bool {
	return p.UserID == SystemUser
}

// System is the principal for internal work in one workspace: the owner
// role, as internal callers carry (identity's ResolveSecret requires it).
func System(workspace uuid.UUID) Principal {
	return Principal{UserID: SystemUser, WorkspaceID: workspace, Role: Owner}
}

// Outgoing copies the three headers onto a service-to-service call.
func (p Principal) Outgoing(ctx context.Context) context.Context {
	return metadata.NewOutgoingContext(ctx, metadata.Pairs(
		UserHeader, p.UserID,
		WorkspaceHeader, p.WorkspaceID.String(),
		RoleHeader, p.Role.String(),
	))
}

// Incoming builds the metadata of an incoming call (tests, and in-process
// callers that go through the gRPC handlers).
func (p Principal) Incoming(ctx context.Context) context.Context {
	return metadata.NewIncomingContext(ctx, metadata.Pairs(
		UserHeader, p.UserID,
		WorkspaceHeader, p.WorkspaceID.String(),
		RoleHeader, p.Role.String(),
	))
}

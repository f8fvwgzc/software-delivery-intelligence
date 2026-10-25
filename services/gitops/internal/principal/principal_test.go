package principal

import (
	"context"
	"testing"

	"github.com/google/uuid"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/metadata"
	"google.golang.org/grpc/status"
)

func TestRoundTripThroughMetadata(t *testing.T) {
	p := Principal{UserID: "u1", WorkspaceID: uuid.New(), Role: Approver}
	out := p.Outgoing(context.Background())
	md, _ := metadata.FromOutgoingContext(out)
	in := metadata.NewIncomingContext(context.Background(), md)
	got, err := FromIncoming(in)
	if err != nil || got != p {
		t.Fatalf("got %+v, %v", got, err)
	}
	if p.Require(Member) != nil || p.Require(Approver) != nil {
		t.Fatal("the role includes the ones below it")
	}
	if status.Code(p.Require(Admin)) != codes.PermissionDenied {
		t.Fatal("a higher role is PERMISSION_DENIED")
	}
}

func TestMissingOrMalformedMetadataIsUnauthenticated(t *testing.T) {
	ws := uuid.NewString()
	for name, pairs := range map[string][]string{
		"nothing":      nil,
		"no workspace": {UserHeader, "u", RoleHeader, "admin"},
		"no user":      {WorkspaceHeader, ws, RoleHeader, "admin"},
		"no role":      {UserHeader, "u", WorkspaceHeader, ws},
		"bad role":     {UserHeader, "u", WorkspaceHeader, ws, RoleHeader, "root"},
		"not a uuid":   {UserHeader, "u", WorkspaceHeader, "acme", RoleHeader, "admin"},
		"empty values": {UserHeader, "", WorkspaceHeader, ws, RoleHeader, "admin"},
	} {
		ctx := metadata.NewIncomingContext(context.Background(), metadata.Pairs(pairs...))
		if _, err := FromIncoming(ctx); status.Code(err) != codes.Unauthenticated {
			t.Errorf("%s: %v", name, err)
		}
	}
}

func TestRolesAreOrdered(t *testing.T) {
	order := []Role{Viewer, Member, Approver, Admin, Owner}
	for i, r := range order {
		parsed, ok := ParseRole(r.String())
		if !ok || parsed != r {
			t.Errorf("%s does not round-trip", r)
		}
		if i > 0 && !(order[i-1] < r) {
			t.Errorf("%s must be above %s", r, order[i-1])
		}
	}
	sys := System(uuid.New())
	if sys.UserID != SystemUser || sys.Role != Owner || !sys.IsSystem() {
		t.Fatalf("system principal: %+v", sys)
	}
}

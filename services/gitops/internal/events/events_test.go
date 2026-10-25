package events

import (
	"testing"

	"github.com/google/uuid"
)

func TestAFeedOnlySeesItsWorkspaceAndNeverBlocksThePublisher(t *testing.T) {
	hub := NewHub()
	mine, theirs := uuid.New(), uuid.New()
	sub := hub.Subscribe(mine)
	defer sub.Close()
	hub.Application(theirs, uuid.New(), "p", "updated", "synced", nil)
	app := uuid.New()
	hub.Application(mine, app, "p1", "updated", "out_of_sync", map[string]string{"health_status": "healthy"})
	e := <-sub.C
	if e.GetEntity() != "application" || e.GetEntityId() != app.String() || e.GetProjectId() != "p1" ||
		e.GetData()[WorkspaceKey] != mine.String() || e.GetData()["health_status"] != "healthy" || e.GetStatus() != "out_of_sync" {
		t.Fatalf("event %+v", e)
	}
	select {
	case other := <-sub.C:
		t.Fatalf("another workspace's event leaked: %+v", other)
	default:
	}
	// A consumer that does not read loses events instead of blocking.
	for i := 0; i < subscriberBuffer*3; i++ {
		hub.Application(mine, app, "p1", "updated", "synced", nil)
	}
	if len(sub.C) != subscriberBuffer {
		t.Fatalf("buffer holds %d", len(sub.C))
	}
}

func TestSubscribersAreBounded(t *testing.T) {
	hub := NewHub()
	ws := uuid.New()
	var subs []*Subscription
	for i := 0; i < MaxSubscribers; i++ {
		s := hub.Subscribe(ws)
		if s == nil {
			t.Fatalf("subscriber %d refused", i)
		}
		subs = append(subs, s)
	}
	if hub.Subscribe(ws) != nil {
		t.Fatal("one too many feeds was opened")
	}
	subs[0].Close()
	if hub.Subscribe(ws) == nil {
		t.Fatal("a closed feed frees its place")
	}
}

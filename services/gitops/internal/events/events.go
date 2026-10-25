// Package events is the service's platform feed (CONTRACT.md §8, §24.6):
// every application change is published here and WatchEvents streams it to
// the gateway, which merges it with the other services' feeds. Events only
// say what changed; a subscriber that falls behind loses the oldest ones,
// which is harmless because the web app refetches on any of them.
package events

import (
	"sync"
	"time"

	"github.com/google/uuid"
	"google.golang.org/protobuf/types/known/timestamppb"

	commonv1 "github.com/f8fvwgzc/software-delivery-intelligence/services/gitops/gen/sdi/common/v1"
)

// WorkspaceKey is the data key every event carries.
const WorkspaceKey = "workspace_id"

const (
	subscriberBuffer = 64
	// MaxSubscribers bounds open feeds.
	MaxSubscribers = 1024
)

// Hub fans events out to subscribers.
type Hub struct {
	mu   sync.Mutex
	subs map[*Subscription]struct{}
}

// NewHub is an empty hub.
func NewHub() *Hub {
	return &Hub{subs: map[*Subscription]struct{}{}}
}

// Subscription receives one workspace's events.
type Subscription struct {
	C         chan *commonv1.PlatformEvent
	workspace string
	hub       *Hub
}

// Subscribe opens a feed for a workspace; nil when too many are open.
func (h *Hub) Subscribe(workspace uuid.UUID) *Subscription {
	h.mu.Lock()
	defer h.mu.Unlock()
	if len(h.subs) >= MaxSubscribers {
		return nil
	}
	s := &Subscription{C: make(chan *commonv1.PlatformEvent, subscriberBuffer), workspace: workspace.String(), hub: h}
	h.subs[s] = struct{}{}
	return s
}

// Close ends a feed.
func (s *Subscription) Close() {
	s.hub.mu.Lock()
	defer s.hub.mu.Unlock()
	delete(s.hub.subs, s)
}

// Publish delivers an event to the feeds of its workspace, never blocking.
func (h *Hub) Publish(e *commonv1.PlatformEvent) {
	h.mu.Lock()
	defer h.mu.Unlock()
	workspace := e.GetData()[WorkspaceKey]
	for s := range h.subs {
		if s.workspace != workspace {
			continue
		}
		select {
		case s.C <- e:
		default:
			// A slow consumer loses this one; the next catches it up.
		}
	}
}

// Application publishes an application change: action is "created",
// "updated", "deleted" or "progress", status is its sync status (or the
// operation phase for progress).
func (h *Hub) Application(workspace, id uuid.UUID, projectID, action, status string, data map[string]string) {
	out := map[string]string{WorkspaceKey: workspace.String()}
	for k, v := range data {
		out[k] = v
	}
	h.Publish(&commonv1.PlatformEvent{
		Entity:    "application",
		EntityId:  id.String(),
		ProjectId: projectID,
		Action:    action,
		Status:    status,
		At:        timestamppb.New(time.Now()),
		Data:      out,
	})
}

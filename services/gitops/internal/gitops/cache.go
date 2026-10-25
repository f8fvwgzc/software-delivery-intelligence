package gitops

import (
	"container/list"
	"sync"
	"time"

	"k8s.io/apimachinery/pkg/apis/meta/v1/unstructured"
)

// ManifestCache keeps rendered manifests by (source, commit) for a while
// (CONTRACT.md §24.3: 24 h, bounded), so a reconcile does not download and
// render an unchanged commit again. Bounded by entries and by bytes; the
// least recently used entry goes first.
type ManifestCache struct {
	mu       sync.Mutex
	ttl      time.Duration
	maxItems int
	maxBytes int
	bytes    int
	order    *list.List
	items    map[string]*list.Element
}

type cached struct {
	key     string
	objects []*unstructured.Unstructured
	kind    string
	size    int
	expires time.Time
}

const (
	ManifestTTL      = 24 * time.Hour
	ManifestMaxItems = 256
	ManifestMaxBytes = 256 << 20
)

// NewManifestCache is an empty cache.
func NewManifestCache(ttl time.Duration, maxItems, maxBytes int) *ManifestCache {
	return &ManifestCache{ttl: ttl, maxItems: maxItems, maxBytes: maxBytes, order: list.New(), items: map[string]*list.Element{}}
}

// Get is a copy of the cached render, if fresh.
func (c *ManifestCache) Get(key string) ([]*unstructured.Unstructured, string, bool) {
	c.mu.Lock()
	defer c.mu.Unlock()
	el, ok := c.items[key]
	if !ok {
		return nil, "", false
	}
	entry := el.Value.(*cached)
	if time.Now().After(entry.expires) {
		c.removeLocked(el)
		return nil, "", false
	}
	c.order.MoveToFront(el)
	return deepCopy(entry.objects), entry.kind, true
}

// Put stores a copy of a render; one larger than the whole budget is not
// kept.
func (c *ManifestCache) Put(key, kind string, objects []*unstructured.Unstructured, size int) {
	if size > c.maxBytes {
		return
	}
	c.mu.Lock()
	defer c.mu.Unlock()
	if el, ok := c.items[key]; ok {
		c.removeLocked(el)
	}
	entry := &cached{key: key, objects: deepCopy(objects), kind: kind, size: size, expires: time.Now().Add(c.ttl)}
	c.items[key] = c.order.PushFront(entry)
	c.bytes += size
	for c.order.Len() > c.maxItems || c.bytes > c.maxBytes {
		c.removeLocked(c.order.Back())
	}
}

// Delete drops one entry (a hard refresh).
func (c *ManifestCache) Delete(key string) {
	c.mu.Lock()
	defer c.mu.Unlock()
	if el, ok := c.items[key]; ok {
		c.removeLocked(el)
	}
}

// DeletePrefix drops every entry whose key starts with prefix (a deleted
// workspace).
func (c *ManifestCache) DeletePrefix(prefix string) {
	c.mu.Lock()
	defer c.mu.Unlock()
	for key, el := range c.items {
		if len(key) >= len(prefix) && key[:len(prefix)] == prefix {
			c.removeLocked(el)
		}
	}
}

// Len and Bytes describe the cache (tests, diagnostics).
func (c *ManifestCache) Len() int {
	c.mu.Lock()
	defer c.mu.Unlock()
	return c.order.Len()
}

func (c *ManifestCache) Bytes() int {
	c.mu.Lock()
	defer c.mu.Unlock()
	return c.bytes
}

func (c *ManifestCache) removeLocked(el *list.Element) {
	entry := el.Value.(*cached)
	c.order.Remove(el)
	delete(c.items, entry.key)
	c.bytes -= entry.size
}

func deepCopy(objs []*unstructured.Unstructured) []*unstructured.Unstructured {
	out := make([]*unstructured.Unstructured, len(objs))
	for i, o := range objs {
		out[i] = o.DeepCopy()
	}
	return out
}

package relay

import (
	"context"
	"crypto/sha256"
	"sync"
	"time"

	"github.com/libp2p/go-libp2p/core/peer"
)

// verifyCache short-circuits bcrypt verification for repeat (peer, room,
// password) tuples for a bounded TTL. Misses are not cached.
type verifyCache struct {
	mu    sync.Mutex
	ttl   time.Duration
	items map[verifyKey]time.Time
}

type verifyKey struct {
	peer       peer.ID
	room       string
	pwDigest32 [32]byte
}

func newVerifyCache(ttl time.Duration) *verifyCache {
	return &verifyCache{
		ttl:   ttl,
		items: make(map[verifyKey]time.Time),
	}
}

func (c *verifyCache) key(p peer.ID, room, password string) verifyKey {
	return verifyKey{peer: p, room: room, pwDigest32: sha256.Sum256([]byte(password))}
}

// Hit returns true if a non-expired entry exists for (p, room, password).
func (c *verifyCache) Hit(p peer.ID, room, password string) bool {
	k := c.key(p, room, password)
	c.mu.Lock()
	defer c.mu.Unlock()
	exp, ok := c.items[k]
	if !ok {
		return false
	}
	if time.Now().After(exp) {
		delete(c.items, k)
		return false
	}
	return true
}

// Store records a successful verification.
func (c *verifyCache) Store(p peer.ID, room, password string) {
	k := c.key(p, room, password)
	c.mu.Lock()
	defer c.mu.Unlock()
	c.items[k] = time.Now().Add(c.ttl)
}

// sweep evicts expired entries. Cheap enough to call periodically.
func (c *verifyCache) sweep() {
	now := time.Now()
	c.mu.Lock()
	defer c.mu.Unlock()
	for k, exp := range c.items {
		if now.After(exp) {
			delete(c.items, k)
		}
	}
}

func (c *verifyCache) size() int {
	c.mu.Lock()
	defer c.mu.Unlock()
	return len(c.items)
}

// runSweeper drains the cache on a tick until ctx is cancelled.
func (c *verifyCache) runSweeper(ctx context.Context, interval time.Duration) {
	t := time.NewTicker(interval)
	defer t.Stop()
	for {
		select {
		case <-ctx.Done():
			return
		case <-t.C:
			c.sweep()
		}
	}
}

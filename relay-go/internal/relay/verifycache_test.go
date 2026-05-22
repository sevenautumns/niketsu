package relay

import (
	"testing"
	"time"

	"github.com/libp2p/go-libp2p/core/peer"
)

func TestVerifyCache_HitMissExpiry(t *testing.T) {
	c := newVerifyCache(50 * time.Millisecond)
	p := peer.ID("p1")
	const room = "r"
	const pw = "deadbeef"

	if c.Hit(p, room, pw) {
		t.Fatal("empty cache must miss")
	}
	c.Store(p, room, pw)
	if !c.Hit(p, room, pw) {
		t.Fatal("immediate lookup must hit")
	}
	if c.Hit(p, room, "different") {
		t.Fatal("different password must miss")
	}
	if c.Hit(peer.ID("p2"), room, pw) {
		t.Fatal("different peer must miss")
	}

	time.Sleep(75 * time.Millisecond)
	if c.Hit(p, room, pw) {
		t.Fatal("expired entry must miss")
	}
}

func TestVerifyCache_Sweep(t *testing.T) {
	c := newVerifyCache(10 * time.Millisecond)
	c.Store(peer.ID("p1"), "r", "pw")
	c.Store(peer.ID("p2"), "r", "pw")
	time.Sleep(20 * time.Millisecond)
	c.sweep()
	if c.size() != 0 {
		t.Fatalf("sweep should have removed expired entries, size=%d", c.size())
	}
}

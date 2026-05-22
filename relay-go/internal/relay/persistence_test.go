package relay

import (
	"encoding/json"
	"os"
	"path/filepath"
	"testing"

	"github.com/libp2p/go-libp2p/core/peer"
)

const testPeerB58 = "12D3KooWBhAY8GMTNyXfsiP9k3yQYUg3yj4KaUaG4nXjyu5gn2bN"

func TestPersistence_WriteAndLoad(t *testing.T) {
	dir := t.TempDir()
	path := filepath.Join(dir, "rooms.json")

	pid, _ := peer.Decode(testPeerB58)
	rs := newRooms()
	rs.byName["room1"] = roomEntry{host: pid, passwordHash: []byte("$2a$10$abc")}
	rs.byHost[pid] = "room1"

	if err := writePersistedRooms(path, rs.snapshot()); err != nil {
		t.Fatal(err)
	}

	snap, err := readPersistedRooms(path)
	if err != nil {
		t.Fatal(err)
	}
	rs2 := newRooms()
	if err := rs2.restore(snap); err != nil {
		t.Fatal(err)
	}
	if rs2.Count() != 1 {
		t.Fatalf("want 1 room after restore, got %d", rs2.Count())
	}
	entry, ok := rs2.byName["room1"]
	if !ok {
		t.Fatal("room1 missing after restore")
	}
	if entry.host != pid {
		t.Fatalf("host mismatch: %s != %s", entry.host, pid)
	}
}

func TestPersistence_LoadMissingFile(t *testing.T) {
	dir := t.TempDir()
	path := filepath.Join(dir, "absent.json")
	snap, err := readPersistedRooms(path)
	if err != nil {
		t.Fatalf("missing file should be a soft error returning empty, got %v", err)
	}
	if len(snap) != 0 {
		t.Fatalf("missing file should yield empty snapshot, got %d", len(snap))
	}
}

func TestPersistence_LoadCorruptFile(t *testing.T) {
	dir := t.TempDir()
	path := filepath.Join(dir, "rooms.json")
	if err := os.WriteFile(path, []byte("not json"), 0644); err != nil {
		t.Fatal(err)
	}
	_, err := readPersistedRooms(path)
	if err == nil {
		t.Fatal("corrupt JSON must return an error so caller can warn")
	}
}

func TestPersistence_AtomicReplaceOnExisting(t *testing.T) {
	dir := t.TempDir()
	path := filepath.Join(dir, "rooms.json")
	if err := os.WriteFile(path, []byte(`[{"room":"old","host_peer_id":"12D3KooWBhAY8GMTNyXfsiP9k3yQYUg3yj4KaUaG4nXjyu5gn2bN","password_hash":""}]`), 0644); err != nil {
		t.Fatal(err)
	}
	pid, _ := peer.Decode(testPeerB58)
	if err := writePersistedRooms(path, []persistedRoom{{
		Room: "new", HostPeerID: pid.String(), PasswordHash: []byte("h"),
	}}); err != nil {
		t.Fatal(err)
	}
	data, _ := os.ReadFile(path)
	var got []persistedRoom
	if err := json.Unmarshal(data, &got); err != nil {
		t.Fatal(err)
	}
	if len(got) != 1 || got[0].Room != "new" {
		t.Fatalf("expected new content, got %+v", got)
	}
}

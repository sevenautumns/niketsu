// Package relay assembles the libp2p host and runs the niketsu auth/relay
// service. Subpackages handle room registry, persistence, metrics, etc.
package relay

import (
	"errors"
	"sync"

	"github.com/libp2p/go-libp2p/core/peer"
	"golang.org/x/crypto/bcrypt"
)

// Sentinel errors returned from rooms operations.
var (
	ErrWrongPassword = errors.New("wrong password")
	ErrNotHost       = errors.New("requesting peer is not the room host")
)

// JoinResult describes the outcome of a successful Join.
type JoinResult struct {
	// Created is true when the room did not exist and was created by this call.
	// The caller is now the host.
	Created bool
	// Host is the current host of the room. When Created is true, Host equals
	// the caller's peer.ID.
	Host peer.ID
}

// rooms is the in-memory room registry. The zero value is not usable; use
// newRooms.
type rooms struct {
	mu     sync.RWMutex
	byName map[string]roomEntry
	byHost map[peer.ID]string
	// onChange is called every time the registry mutates. Persistence wires
	// here to schedule a debounced flush.
	onChange func()
}

type roomEntry struct {
	host         peer.ID
	passwordHash []byte // bcrypt hash
}

func newRooms() *rooms {
	return &rooms{
		byName: make(map[string]roomEntry),
		byHost: make(map[peer.ID]string),
	}
}

// Join attempts to admit caller to room with the given password. password is
// the client-pre-hashed token (hex of SHA-256), matching the Rust client.
// Returns ErrWrongPassword on a bad password.
//
// IMPORTANT: bcrypt is CPU-bound. This function takes locks only to read out
// the stored hash and only takes the write lock to mutate the maps. bcrypt
// never runs while holding any lock.
func (r *rooms) Join(caller peer.ID, room, password string) (JoinResult, error) {
	r.mu.RLock()
	entry, exists := r.byName[room]
	r.mu.RUnlock()

	if exists {
		if err := bcrypt.CompareHashAndPassword(entry.passwordHash, []byte(password)); err != nil {
			return JoinResult{}, ErrWrongPassword
		}
		return JoinResult{Created: false, Host: entry.host}, nil
	}

	hash, err := bcrypt.GenerateFromPassword([]byte(password), bcrypt.DefaultCost)
	if err != nil {
		return JoinResult{}, err
	}

	r.mu.Lock()
	if entry, exists := r.byName[room]; exists {
		r.mu.Unlock()
		if err := bcrypt.CompareHashAndPassword(entry.passwordHash, []byte(password)); err != nil {
			return JoinResult{}, ErrWrongPassword
		}
		return JoinResult{Created: false, Host: entry.host}, nil
	}
	r.byName[room] = roomEntry{host: caller, passwordHash: hash}
	r.byHost[caller] = room
	r.mu.Unlock()

	r.notifyChange()
	return JoinResult{Created: true, Host: caller}, nil
}

// Transfer swaps the host of room to newHost, only if caller is the current
// host. Returns ErrNotHost otherwise.
func (r *rooms) Transfer(caller peer.ID, room string, newHost peer.ID) error {
	r.mu.Lock()
	defer r.mu.Unlock()

	entry, ok := r.byName[room]
	if !ok {
		return ErrNotHost
	}
	if entry.host != caller {
		return ErrNotHost
	}
	delete(r.byHost, caller)
	r.byHost[newHost] = room
	entry.host = newHost
	r.byName[room] = entry
	r.notifyChangeLocked()
	return nil
}

// OnDisconnect removes the room owned by peer, if any.
func (r *rooms) OnDisconnect(p peer.ID) {
	r.mu.Lock()
	defer r.mu.Unlock()

	room, ok := r.byHost[p]
	if !ok {
		return
	}
	delete(r.byHost, p)
	delete(r.byName, room)
	r.notifyChangeLocked()
}

// Count returns the number of rooms currently registered.
func (r *rooms) Count() int {
	r.mu.RLock()
	defer r.mu.RUnlock()
	return len(r.byName)
}

// LookupHost returns the current host of room if it exists.
func (r *rooms) LookupHost(room string) (peer.ID, bool) {
	r.mu.RLock()
	defer r.mu.RUnlock()
	e, ok := r.byName[room]
	if !ok {
		return "", false
	}
	return e.host, true
}

// snapshot returns a deep copy of all room entries.
func (r *rooms) snapshot() []persistedRoom {
	r.mu.RLock()
	defer r.mu.RUnlock()
	out := make([]persistedRoom, 0, len(r.byName))
	for name, e := range r.byName {
		out = append(out, persistedRoom{
			Room:         name,
			HostPeerID:   e.host.String(),
			PasswordHash: append([]byte(nil), e.passwordHash...),
		})
	}
	return out
}

// restore replaces the registry's contents with the given snapshot.
func (r *rooms) restore(snap []persistedRoom) error {
	r.mu.Lock()
	defer r.mu.Unlock()
	r.byName = make(map[string]roomEntry, len(snap))
	r.byHost = make(map[peer.ID]string, len(snap))
	for _, p := range snap {
		pid, err := peer.Decode(p.HostPeerID)
		if err != nil {
			return err
		}
		r.byName[p.Room] = roomEntry{host: pid, passwordHash: p.PasswordHash}
		r.byHost[pid] = p.Room
	}
	return nil
}

// notifyChange fires onChange if set. Caller must NOT hold the lock.
func (r *rooms) notifyChange() {
	if r.onChange != nil {
		r.onChange()
	}
}

// notifyChangeLocked is for callers that already hold r.mu.
func (r *rooms) notifyChangeLocked() {
	if r.onChange != nil {
		go r.onChange()
	}
}

// persistedRoom is the serialisation shape for one room. Defined here so
// rooms.snapshot/restore can construct it without importing the persistence
// package.
type persistedRoom struct {
	Room         string `json:"room"`
	HostPeerID   string `json:"host_peer_id"`
	PasswordHash []byte `json:"password_hash"`
}

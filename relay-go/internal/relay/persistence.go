package relay

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io/fs"
	"log/slog"
	"os"
	"path/filepath"
	"time"
)

// readPersistedRooms loads the room snapshot from path. A missing file is
// treated as an empty snapshot (no error). A corrupt file returns an error so
// the caller can decide what to do (we log + start fresh).
func readPersistedRooms(path string) ([]persistedRoom, error) {
	data, err := os.ReadFile(path)
	if err != nil {
		if errors.Is(err, fs.ErrNotExist) {
			return nil, nil
		}
		return nil, err
	}
	var out []persistedRoom
	if err := json.Unmarshal(data, &out); err != nil {
		return nil, fmt.Errorf("decode %s: %w", path, err)
	}
	return out, nil
}

// writePersistedRooms writes snap to path atomically via tmp + rename.
func writePersistedRooms(path string, snap []persistedRoom) error {
	if err := os.MkdirAll(filepath.Dir(path), 0755); err != nil {
		return err
	}
	tmp := path + ".tmp"
	data, err := json.MarshalIndent(snap, "", "  ")
	if err != nil {
		return err
	}
	if err := os.WriteFile(tmp, data, 0644); err != nil {
		return err
	}
	return os.Rename(tmp, path)
}

// persistor runs a debounced flush loop. It owns the dirty channel; mutators
// call (*persistor).Notify() to schedule a flush.
type persistor struct {
	path     string
	rooms    *rooms
	interval time.Duration
	dirty    chan struct{}
}

func newPersistor(path string, r *rooms, flushInterval time.Duration) *persistor {
	return &persistor{
		path:     path,
		rooms:    r,
		interval: flushInterval,
		dirty:    make(chan struct{}, 1),
	}
}

// Notify schedules a flush; coalesces multiple notifications into one.
func (p *persistor) Notify() {
	select {
	case p.dirty <- struct{}{}:
	default:
	}
}

// Run drains the dirty channel until ctx is cancelled, debouncing writes by
// interval. Writes one final snapshot on shutdown.
func (p *persistor) Run(ctx context.Context) {
	for {
		select {
		case <-ctx.Done():
			p.flush()
			return
		case <-p.dirty:
			t := time.NewTimer(p.interval)
			select {
			case <-ctx.Done():
				t.Stop()
				p.flush()
				return
			case <-t.C:
			}
			select {
			case <-p.dirty:
			default:
			}
			p.flush()
		}
	}
}

func (p *persistor) flush() {
	if err := writePersistedRooms(p.path, p.rooms.snapshot()); err != nil {
		slog.Warn("persistence flush failed", "path", p.path, "err", err)
	}
}

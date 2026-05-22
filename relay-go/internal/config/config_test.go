package config

import (
	"os"
	"path/filepath"
	"testing"
)

func TestLoadOrDefault_NoFile_ReturnsDefaults(t *testing.T) {
	dir := t.TempDir()
	path := filepath.Join(dir, "config.toml")
	cfg := LoadOrDefaultAt(path)
	if cfg.Port != 7766 {
		t.Errorf("Port: want 7766, got %d", cfg.Port)
	}
	if cfg.Persistence.Enabled {
		t.Error("Persistence.Enabled: want false by default")
	}
	if cfg.Metrics.Enabled {
		t.Error("Metrics.Enabled: want false by default")
	}
	if cfg.VerifyCache.TTLSeconds != 30 {
		t.Errorf("VerifyCache.TTLSeconds: want 30, got %d", cfg.VerifyCache.TTLSeconds)
	}
}

func TestSaveLoadRoundTrip(t *testing.T) {
	dir := t.TempDir()
	path := filepath.Join(dir, "config.toml")
	want := Config{
		Port:    9999,
		Keypair: "BASE64-PROTOBUF-BYTES",
	}
	want.Persistence.Enabled = true
	want.Persistence.Path = "rooms.json"
	want.Persistence.FlushIntervalMs = 500
	want.Metrics.Enabled = true
	want.Metrics.Addr = "127.0.0.1:9090"
	want.VerifyCache.TTLSeconds = 30

	if err := want.SaveTo(path); err != nil {
		t.Fatalf("SaveTo: %v", err)
	}
	got := LoadOrDefaultAt(path)
	if got != want {
		t.Errorf("round-trip mismatch:\nwant: %#v\ngot:  %#v", want, got)
	}
}

func TestLoadOrDefault_CorruptFile_ReturnsDefaults(t *testing.T) {
	dir := t.TempDir()
	path := filepath.Join(dir, "config.toml")
	if err := os.WriteFile(path, []byte("not valid toml @@@"), 0644); err != nil {
		t.Fatal(err)
	}
	cfg := LoadOrDefaultAt(path)
	if cfg.Port != 7766 {
		t.Errorf("corrupt file should yield defaults, got Port=%d", cfg.Port)
	}
}

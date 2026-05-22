package config

import (
	"path/filepath"
	"runtime"
	"strings"
	"testing"
)

func TestConfigPathHasProjectSuffix(t *testing.T) {
	got, err := ConfigPath()
	if err != nil {
		t.Fatalf("ConfigPath() error: %v", err)
	}
	if filepath.Base(got) != "config.toml" {
		t.Fatalf("expected filename config.toml, got %q", filepath.Base(got))
	}

	dir := filepath.Dir(got)
	var suffix string
	switch runtime.GOOS {
	case "darwin":
		suffix = "de.autumnal.niketsu-relay"
	case "windows":
		suffix = filepath.Join("autumnal", "niketsu-relay", "config")
	default: // linux, others
		suffix = "niketsu-relay"
	}
	if !strings.HasSuffix(dir, suffix) {
		t.Fatalf("expected dir to end with %q, got %q", suffix, dir)
	}
}

func TestCachePathHasProjectSuffix(t *testing.T) {
	got, err := CachePath()
	if err != nil {
		t.Fatalf("CachePath() error: %v", err)
	}
	if filepath.Base(got) != "niketsu-relay.log" {
		t.Fatalf("expected filename niketsu-relay.log, got %q", filepath.Base(got))
	}
}

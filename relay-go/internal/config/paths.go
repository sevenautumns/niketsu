// Package config provides loading, saving and path resolution for the
// niketsu-relay configuration file.
package config

import (
	"fmt"
	"os"
	"path/filepath"
	"runtime"
)

const (
	configFile = "config.toml"
	logFile    = "niketsu-relay.log"
)

// projectDir resolves the per-platform directory the Rust `directories` crate
// would produce for ProjectDirs::from("de", "autumnal", "niketsu-relay").
// kind is either "config" or "cache".
func projectDir(kind string) (string, error) {
	switch runtime.GOOS {
	case "darwin":
		home, err := os.UserHomeDir()
		if err != nil {
			return "", err
		}
		switch kind {
		case "config":
			return filepath.Join(home, "Library", "Application Support", "de.autumnal.niketsu-relay"), nil
		case "cache":
			return filepath.Join(home, "Library", "Caches", "de.autumnal.niketsu-relay"), nil
		}
	case "windows":
		switch kind {
		case "config":
			appData := os.Getenv("APPDATA")
			if appData == "" {
				return "", fmt.Errorf("APPDATA not set")
			}
			return filepath.Join(appData, "autumnal", "niketsu-relay", "config"), nil
		case "cache":
			local := os.Getenv("LOCALAPPDATA")
			if local == "" {
				return "", fmt.Errorf("LOCALAPPDATA not set")
			}
			return filepath.Join(local, "autumnal", "niketsu-relay", "cache"), nil
		}
	default: // linux et al — XDG
		switch kind {
		case "config":
			base, err := os.UserConfigDir()
			if err != nil {
				return "", err
			}
			return filepath.Join(base, "niketsu-relay"), nil
		case "cache":
			base, err := os.UserCacheDir()
			if err != nil {
				return "", err
			}
			return filepath.Join(base, "niketsu-relay"), nil
		}
	}
	return "", fmt.Errorf("unsupported kind: %s", kind)
}

// ConfigPath returns the absolute path to config.toml on this platform.
func ConfigPath() (string, error) {
	dir, err := projectDir("config")
	if err != nil {
		return "", err
	}
	return filepath.Join(dir, configFile), nil
}

// CachePath returns the absolute path to the log file on this platform.
func CachePath() (string, error) {
	dir, err := projectDir("cache")
	if err != nil {
		return "", err
	}
	return filepath.Join(dir, logFile), nil
}

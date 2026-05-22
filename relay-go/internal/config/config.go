package config

import (
	"fmt"
	"os"
	"path/filepath"

	"github.com/BurntSushi/toml"
)

const (
	defaultPort        uint16 = 7766
	defaultVerifyTTLS         = 30
	defaultMetricsAddr        = "127.0.0.1:9090"
	defaultPersistPath        = "rooms.json"
	defaultFlushIntMs         = 500
)

// Config is the on-disk relay configuration. It is loaded from TOML.
type Config struct {
	// Port the libp2p host listens on (TCP and UDP/QUIC). Default 7766.
	Port uint16 `toml:"port"`
	// Keypair is the libp2p private key serialised via crypto.MarshalPrivateKey
	// (protobuf bytes), then base64-encoded. Empty on first run; generated and
	// saved back automatically.
	Keypair string `toml:"keypair,omitempty"`

	Persistence PersistenceConfig `toml:"persistence"`
	Metrics     MetricsConfig     `toml:"metrics"`
	VerifyCache VerifyCacheConfig `toml:"verify_cache"`
}

type PersistenceConfig struct {
	Enabled         bool   `toml:"enabled"`
	Path            string `toml:"path"`
	FlushIntervalMs int    `toml:"flush_interval_ms"`
}

type MetricsConfig struct {
	Enabled bool   `toml:"enabled"`
	Addr    string `toml:"addr"`
}

type VerifyCacheConfig struct {
	TTLSeconds int `toml:"ttl_seconds"`
}

// defaults returns a Config populated with the same defaults the Rust relay
// uses, plus defaults for the Go-only additions.
func defaults() Config {
	return Config{
		Port: defaultPort,
		Persistence: PersistenceConfig{
			Enabled:         false,
			Path:            defaultPersistPath,
			FlushIntervalMs: defaultFlushIntMs,
		},
		Metrics: MetricsConfig{
			Enabled: false,
			Addr:    defaultMetricsAddr,
		},
		VerifyCache: VerifyCacheConfig{
			TTLSeconds: defaultVerifyTTLS,
		},
	}
}

// LoadOrDefault loads config from the platform's config path, falling back to
// defaults if the file is missing or unreadable.
func LoadOrDefault() Config {
	path, err := ConfigPath()
	if err != nil {
		return defaults()
	}
	return LoadOrDefaultAt(path)
}

// LoadOrDefaultAt is LoadOrDefault but with the path overridden, for testing.
func LoadOrDefaultAt(path string) Config {
	cfg := defaults()
	data, err := os.ReadFile(path)
	if err != nil {
		return cfg
	}
	if err := toml.Unmarshal(data, &cfg); err != nil {
		return defaults()
	}
	return cfg
}

// Save writes the config to the platform's config path, creating parent dirs.
func (c Config) Save() error {
	path, err := ConfigPath()
	if err != nil {
		return err
	}
	return c.SaveTo(path)
}

// SaveTo writes the config to the given path, creating parent dirs.
func (c Config) SaveTo(path string) error {
	if err := os.MkdirAll(filepath.Dir(path), 0755); err != nil {
		return fmt.Errorf("mkdir: %w", err)
	}
	f, err := os.Create(path)
	if err != nil {
		return fmt.Errorf("create: %w", err)
	}
	defer f.Close()
	return toml.NewEncoder(f).Encode(c)
}

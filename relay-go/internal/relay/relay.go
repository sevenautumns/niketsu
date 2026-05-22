package relay

import (
	"context"
	"encoding/base64"
	"fmt"
	"log/slog"
	"path/filepath"
	"sync"
	"time"

	"github.com/libp2p/go-libp2p"
	"github.com/libp2p/go-libp2p/core/crypto"
	"github.com/libp2p/go-libp2p/core/host"
	"github.com/libp2p/go-libp2p/core/network"
	libp2prelay "github.com/libp2p/go-libp2p/p2p/protocol/circuitv2/relay"
	"github.com/prometheus/client_golang/prometheus"
	"github.com/sevenautumns/niketsu/relay-go/internal/config"
)

const authProtocolID = "/authorisation/1"

// Relay is the assembled, runnable instance.
type Relay struct {
	cfg        config.Config
	configPath string
	host       host.Host
	rooms      *rooms
	cache      *verifyCache
	auth       *authService
	metrics    *metrics
	metricsSrv interface {
		Shutdown(context.Context) error
	}
	persistor *persistor
	pollWG    sync.WaitGroup
}

// New builds a Relay from cfg. If cfg.Keypair is empty, a fresh Ed25519
// keypair is generated and cfg is mutated to include it; the caller can
// then call cfg.Save() to persist. configPath is used only for the save-back.
func New(cfg config.Config, configPath string) (*Relay, *config.Config, error) {
	priv, cfgWithKey, err := loadOrCreateKey(cfg)
	if err != nil {
		return nil, nil, fmt.Errorf("keypair: %w", err)
	}

	listenAddrs := []string{
		fmt.Sprintf("/ip4/0.0.0.0/tcp/%d", cfg.Port),
		fmt.Sprintf("/ip6/::/tcp/%d", cfg.Port),
		fmt.Sprintf("/ip4/0.0.0.0/udp/%d/quic-v1", cfg.Port),
		fmt.Sprintf("/ip6/::/udp/%d/quic-v1", cfg.Port),
	}

	h, err := libp2p.New(
		libp2p.Identity(priv),
		libp2p.ListenAddrStrings(listenAddrs...),
		libp2p.Ping(true),
		libp2p.UserAgent("niketsu-relay-go/0.1.0"),
	)
	if err != nil {
		return nil, nil, fmt.Errorf("libp2p host: %w", err)
	}

	// Circuit relay v2 server. Uses go-libp2p's DefaultResources() (2 min
	// circuit duration, 128 KiB byte cap), matching the Rust relay's
	// `relay::Config::default()` after the chunk-streaming fix was reverted.
	_, err = libp2prelay.New(h)
	if err != nil {
		_ = h.Close()
		return nil, nil, fmt.Errorf("circuit relay: %w", err)
	}

	rs := newRooms()
	cache := newVerifyCache(time.Duration(cfg.VerifyCache.TTLSeconds) * time.Second)

	var m *metrics
	var reg *prometheus.Registry
	if cfg.Metrics.Enabled {
		reg = prometheus.NewRegistry()
		m = newMetrics(reg)
	} else {
		m = newNopMetrics()
	}

	auth := &authService{rooms: rs, cache: cache, metrics: m}

	h.Network().Notify(&disconnectNotifiee{rooms: rs})

	h.SetStreamHandler(authProtocolID, func(s network.Stream) {
		defer s.Close()
		auth.handleStream(s.Conn().RemotePeer(), s)
	})

	r := &Relay{
		cfg:        cfgWithKey,
		configPath: configPath,
		host:       h,
		rooms:      rs,
		cache:      cache,
		auth:       auth,
		metrics:    m,
	}

	if cfg.Persistence.Enabled {
		persistPath := cfg.Persistence.Path
		if !filepath.IsAbs(persistPath) {
			persistPath = filepath.Join(filepath.Dir(configPath), persistPath)
		}
		snap, err := readPersistedRooms(persistPath)
		if err != nil {
			slog.Warn("could not load persisted rooms; starting empty", "path", persistPath, "err", err)
		} else if err := rs.restore(snap); err != nil {
			slog.Warn("could not restore persisted rooms; starting empty", "path", persistPath, "err", err)
		}
		r.persistor = newPersistor(persistPath, rs, time.Duration(cfg.Persistence.FlushIntervalMs)*time.Millisecond)
		rs.onChange = r.persistor.Notify
	}

	if cfg.Metrics.Enabled {
		r.metricsSrv = startMetricsServer(cfg.Metrics.Addr, reg)
	}

	return r, &cfgWithKey, nil
}

// PeerID returns the relay's libp2p peer id.
func (r *Relay) PeerID() string { return r.host.ID().String() }

// ListenAddrs returns the addresses the relay is listening on.
func (r *Relay) ListenAddrs() []string {
	out := make([]string, 0, len(r.host.Addrs()))
	for _, a := range r.host.Addrs() {
		out = append(out, a.String()+"/p2p/"+r.host.ID().String())
	}
	return out
}

// Run blocks until ctx is cancelled, then shuts everything down.
func (r *Relay) Run(ctx context.Context) error {
	r.pollWG.Add(1)
	go func() {
		defer r.pollWG.Done()
		r.cache.runSweeper(ctx, time.Minute)
	}()

	r.pollWG.Add(1)
	go func() {
		defer r.pollWG.Done()
		r.metrics.pollHost(ctx, r.host, r.rooms, 5*time.Second)
	}()

	if r.persistor != nil {
		r.pollWG.Add(1)
		go func() {
			defer r.pollWG.Done()
			r.persistor.Run(ctx)
		}()
	}

	<-ctx.Done()
	return r.shutdown()
}

func (r *Relay) shutdown() error {
	shutdownCtx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()

	if r.metricsSrv != nil {
		_ = r.metricsSrv.Shutdown(shutdownCtx)
	}
	if err := r.host.Close(); err != nil {
		slog.Warn("host close error", "err", err)
	}
	r.pollWG.Wait()
	return nil
}

// loadOrCreateKey decodes cfg.Keypair (base64 of crypto.MarshalPrivateKey).
// If empty, generates a fresh Ed25519 keypair and returns a new Config copy
// with the base64-encoded bytes populated.
func loadOrCreateKey(cfg config.Config) (crypto.PrivKey, config.Config, error) {
	if cfg.Keypair != "" {
		raw, err := base64.StdEncoding.DecodeString(cfg.Keypair)
		if err != nil {
			return nil, cfg, fmt.Errorf("decode base64 keypair: %w", err)
		}
		priv, err := crypto.UnmarshalPrivateKey(raw)
		if err != nil {
			return nil, cfg, fmt.Errorf("unmarshal keypair: %w", err)
		}
		return priv, cfg, nil
	}
	priv, _, err := crypto.GenerateEd25519Key(nil)
	if err != nil {
		return nil, cfg, err
	}
	raw, err := crypto.MarshalPrivateKey(priv)
	if err != nil {
		return nil, cfg, err
	}
	cfg.Keypair = base64.StdEncoding.EncodeToString(raw)
	return priv, cfg, nil
}

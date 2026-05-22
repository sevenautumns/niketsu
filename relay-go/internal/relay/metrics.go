package relay

import (
	"context"
	"net/http"
	"time"

	"github.com/libp2p/go-libp2p/core/host"
	"github.com/prometheus/client_golang/prometheus"
	"github.com/prometheus/client_golang/prometheus/collectors"
	"github.com/prometheus/client_golang/prometheus/promhttp"
)

// metrics holds all Prometheus collectors for the relay. When metrics are
// disabled in config the relay code creates a no-op instance (newNopMetrics).
type metrics struct {
	rooms          prometheus.Gauge
	authRequests   *prometheus.CounterVec
	bcryptDuration *prometheus.HistogramVec
	verifyCache    *prometheus.CounterVec
	connectedPeers prometheus.Gauge
	registry       *prometheus.Registry
	disabled       bool
}

// newMetrics constructs the collectors and registers them on reg. It also
// registers the default Go runtime / process collectors.
func newMetrics(reg *prometheus.Registry) *metrics {
	m := &metrics{registry: reg}
	m.rooms = prometheus.NewGauge(prometheus.GaugeOpts{
		Name: "niketsu_relay_rooms_total",
		Help: "Number of rooms currently registered.",
	})
	m.authRequests = prometheus.NewCounterVec(prometheus.CounterOpts{
		Name: "niketsu_relay_auth_requests_total",
		Help: "Total /authorisation/1 requests by outcome.",
	}, []string{"outcome"})
	m.bcryptDuration = prometheus.NewHistogramVec(prometheus.HistogramOpts{
		Name:    "niketsu_relay_bcrypt_seconds",
		Help:    "Time spent inside bcrypt per operation.",
		Buckets: prometheus.ExponentialBuckets(0.01, 2, 8),
	}, []string{"op"})
	m.verifyCache = prometheus.NewCounterVec(prometheus.CounterOpts{
		Name: "niketsu_relay_verify_cache_total",
		Help: "Verify-cache lookups by result.",
	}, []string{"result"})
	m.connectedPeers = prometheus.NewGauge(prometheus.GaugeOpts{
		Name: "niketsu_relay_connected_peers",
		Help: "Number of peers with at least one active connection.",
	})

	reg.MustRegister(
		m.rooms,
		m.authRequests,
		m.bcryptDuration,
		m.verifyCache,
		m.connectedPeers,
		collectors.NewGoCollector(),
		collectors.NewProcessCollector(collectors.ProcessCollectorOpts{}),
	)
	// Pre-create zero-valued series for known label sets so dashboards show
	// the metric families immediately, before any auth traffic.
	m.bcryptDuration.WithLabelValues("hash")
	m.bcryptDuration.WithLabelValues("verify")
	for _, outcome := range []string{"ok", "err", "transfer"} {
		m.authRequests.WithLabelValues(outcome)
	}
	for _, result := range []string{"hit", "miss"} {
		m.verifyCache.WithLabelValues(result)
	}
	return m
}

func newNopMetrics() *metrics { return &metrics{disabled: true} }

func (m *metrics) AuthRequest(outcome string) {
	if m == nil || m.disabled {
		return
	}
	m.authRequests.WithLabelValues(outcome).Inc()
}

func (m *metrics) VerifyCache(result string) {
	if m == nil || m.disabled {
		return
	}
	m.verifyCache.WithLabelValues(result).Inc()
}

// ObserveBcrypt records a bcrypt operation's duration.
func (m *metrics) ObserveBcrypt(op string, d time.Duration) {
	if m == nil || m.disabled {
		return
	}
	m.bcryptDuration.WithLabelValues(op).Observe(d.Seconds())
}

// SetRooms updates the gauge from the room registry.
func (m *metrics) SetRooms(n int) {
	if m == nil || m.disabled {
		return
	}
	m.rooms.Set(float64(n))
}

// pollHost runs a background loop that updates host-derived gauges every
// interval until ctx is cancelled.
func (m *metrics) pollHost(ctx context.Context, h host.Host, r *rooms, interval time.Duration) {
	if m == nil || m.disabled {
		return
	}
	t := time.NewTicker(interval)
	defer t.Stop()
	for {
		select {
		case <-ctx.Done():
			return
		case <-t.C:
			m.connectedPeers.Set(float64(len(h.Network().Peers())))
			m.SetRooms(r.Count())
		}
	}
}

// startMetricsServer launches an HTTP server on addr that serves /metrics.
func startMetricsServer(addr string, reg *prometheus.Registry) *http.Server {
	mux := http.NewServeMux()
	mux.Handle("/metrics", promhttp.HandlerFor(reg, promhttp.HandlerOpts{Registry: reg}))
	srv := &http.Server{Addr: addr, Handler: mux, ReadHeaderTimeout: 5 * time.Second}
	go func() {
		_ = srv.ListenAndServe()
	}()
	return srv
}

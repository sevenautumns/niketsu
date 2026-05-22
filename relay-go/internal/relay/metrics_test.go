package relay

import (
	"strings"
	"testing"

	"github.com/prometheus/client_golang/prometheus"
	"github.com/prometheus/client_golang/prometheus/testutil"
)

func TestMetrics_AuthRequestCounter(t *testing.T) {
	reg := prometheus.NewRegistry()
	m := newMetrics(reg)
	m.AuthRequest("ok")
	m.AuthRequest("ok")
	m.AuthRequest("err")

	got := testutil.ToFloat64(m.authRequests.WithLabelValues("ok"))
	if got != 2 {
		t.Fatalf("authRequests{outcome=ok}: want 2, got %v", got)
	}
	got = testutil.ToFloat64(m.authRequests.WithLabelValues("err"))
	if got != 1 {
		t.Fatalf("authRequests{outcome=err}: want 1, got %v", got)
	}
}

func TestMetrics_VerifyCacheCounter(t *testing.T) {
	reg := prometheus.NewRegistry()
	m := newMetrics(reg)
	m.VerifyCache("hit")
	m.VerifyCache("miss")
	m.VerifyCache("miss")

	if hit := testutil.ToFloat64(m.verifyCache.WithLabelValues("hit")); hit != 1 {
		t.Fatalf("verifyCache{result=hit}: want 1, got %v", hit)
	}
	if miss := testutil.ToFloat64(m.verifyCache.WithLabelValues("miss")); miss != 2 {
		t.Fatalf("verifyCache{result=miss}: want 2, got %v", miss)
	}
}

func TestMetrics_ExposesAllExpectedNames(t *testing.T) {
	reg := prometheus.NewRegistry()
	_ = newMetrics(reg)
	mfs, err := reg.Gather()
	if err != nil {
		t.Fatal(err)
	}
	want := []string{
		"niketsu_relay_rooms_total",
		"niketsu_relay_auth_requests_total",
		"niketsu_relay_bcrypt_seconds",
		"niketsu_relay_verify_cache_total",
		"niketsu_relay_connected_peers",
	}
	got := map[string]bool{}
	for _, mf := range mfs {
		got[mf.GetName()] = true
	}
	for _, name := range want {
		if !got[name] {
			t.Errorf("missing metric %q; got: %s", name, strings.Join(keysOf(got), ", "))
		}
	}
}

func keysOf(m map[string]bool) []string {
	out := make([]string, 0, len(m))
	for k := range m {
		out = append(out, k)
	}
	return out
}

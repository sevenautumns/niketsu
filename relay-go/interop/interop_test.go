//go:build interop

// Interop tests against the Rust niketsu client. Run with:
//
//	go test -tags interop ./interop/...
//
// Requires: cargo in PATH, the repo's Rust workspace builds, and the Go
// niketsu-relay binary buildable from the parent module.
package interop

import (
	"bufio"
	"context"
	"fmt"
	"net"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"testing"
	"time"
)

// freePort returns an unused TCP port.
func freePort(t *testing.T) int {
	t.Helper()
	l, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	defer l.Close()
	return l.Addr().(*net.TCPAddr).Port
}

func buildGoRelay(t *testing.T) string {
	t.Helper()
	tmp := t.TempDir()
	out := filepath.Join(tmp, "niketsu-relay")
	cmd := exec.Command("go", "build", "-o", out, "./cmd/niketsu-relay")
	cmd.Dir = ".."
	cmd.Stderr = os.Stderr
	if err := cmd.Run(); err != nil {
		t.Fatalf("go build niketsu-relay: %v", err)
	}
	return out
}

func TestInterop_GoRelayStarts(t *testing.T) {
	if _, err := exec.LookPath("cargo"); err != nil {
		t.Skip("cargo not in PATH")
	}
	port := freePort(t)
	bin := buildGoRelay(t)

	confHome := t.TempDir()
	t.Setenv("XDG_CONFIG_HOME", confHome)
	t.Setenv("XDG_CACHE_HOME", t.TempDir())

	ctx, cancel := context.WithTimeout(context.Background(), 20*time.Second)
	defer cancel()

	cmd := exec.CommandContext(ctx, bin, "-p", fmt.Sprintf("%d", port), "-t", "info")
	stderr, err := cmd.StderrPipe()
	if err != nil {
		t.Fatal(err)
	}
	if err := cmd.Start(); err != nil {
		t.Fatal(err)
	}
	defer func() {
		_ = cmd.Process.Signal(os.Interrupt)
		_ = cmd.Wait()
	}()

	scanner := bufio.NewScanner(stderr)
	deadline := time.After(10 * time.Second)
	started := make(chan string, 1)
	go func() {
		for scanner.Scan() {
			line := scanner.Text()
			if strings.Contains(line, "relay started") {
				started <- line
				return
			}
		}
	}()
	select {
	case <-started:
		// good
	case <-deadline:
		t.Fatal("relay did not log 'relay started' within 10s")
	}
}

// TODO(impl): Add a second test that drives the Rust niketsu client against
// the Go relay through a generated config.toml. Shape:
//
//	1. cargo build --release -p niketsu --bin niketsu (in a t.TempDir cache)
//	2. Generate ~/.config/niketsu/config.toml pointing at the Go relay
//	3. Launch two niketsu instances headlessly (or with --no-ui if available)
//	4. Assert one becomes host, the other connects
//	5. Send a transfer message; observe role swap
//
// The exact mechanism depends on what the niketsu binary exposes for
// scriptable testing. If headless operation isn't available today, document
// this as a manual verification step in the README and leave the smoke test
// above as the only automated interop assertion.

# niketsu-relay (Go)

Go port of the Rust `niketsu-relay`, wire-compatible with existing Rust clients. See `docs/superpowers/specs/2026-05-22-go-relay-port-design.md` for the design.

Build: `go build ./cmd/niketsu-relay`
Test:  `go test ./...`
Interop test (needs Rust toolchain): `go test -tags interop ./interop/...`

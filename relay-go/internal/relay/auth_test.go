package relay

import (
	"bytes"
	"io"
	"testing"
	"time"

	"github.com/libp2p/go-libp2p/core/crypto"
	"github.com/libp2p/go-libp2p/core/peer"
	"github.com/sevenautumns/niketsu/relay-go/internal/wire"
)

type fakeStream struct {
	io.Reader
	io.Writer
}

func newPeer(t *testing.T) peer.ID {
	t.Helper()
	priv, _, err := crypto.GenerateEd25519Key(nil)
	if err != nil {
		t.Fatal(err)
	}
	pid, err := peer.IDFromPrivateKey(priv)
	if err != nil {
		t.Fatal(err)
	}
	return pid
}

func newAuth(t *testing.T) *authService {
	t.Helper()
	return &authService{
		rooms: newRooms(),
		cache: newVerifyCache(time.Second),
	}
}

func writeReq(t *testing.T, req wire.InitRequest) *bytes.Buffer {
	t.Helper()
	var buf bytes.Buffer
	if err := wire.WriteInitRequest(&buf, req); err != nil {
		t.Fatal(err)
	}
	return &buf
}

func readResp(t *testing.T, buf *bytes.Buffer) wire.InitResponse {
	t.Helper()
	r, err := wire.ReadInitResponse(buf)
	if err != nil {
		t.Fatal(err)
	}
	return r
}

func TestAuth_FirstJoinCreatesRoom(t *testing.T) {
	a := newAuth(t)
	hostA := newPeer(t)
	out := &bytes.Buffer{}
	a.handleStream(hostA, fakeStream{
		Reader: writeReq(t, wire.InitRequest{Room: "movie", Password: "deadbeef"}),
		Writer: out,
	})
	resp := readResp(t, out)

	if resp.Status != wire.StatusOk {
		t.Fatalf("status: want Ok, got %s", resp.Status)
	}
	if resp.PeerID != nil {
		t.Fatalf("peer_id: want nil (caller is host), got %s", resp.PeerID)
	}
	if a.rooms.Count() != 1 {
		t.Fatalf("want 1 room, got %d", a.rooms.Count())
	}
}

func TestAuth_SecondJoinSendsHost(t *testing.T) {
	a := newAuth(t)
	hostA, clientB := newPeer(t), newPeer(t)

	a.handleStream(hostA, fakeStream{
		Reader: writeReq(t, wire.InitRequest{Room: "r", Password: "pw"}),
		Writer: &bytes.Buffer{},
	})
	out := &bytes.Buffer{}
	a.handleStream(clientB, fakeStream{
		Reader: writeReq(t, wire.InitRequest{Room: "r", Password: "pw"}),
		Writer: out,
	})
	resp := readResp(t, out)
	if resp.Status != wire.StatusOk {
		t.Fatalf("status: want Ok, got %s", resp.Status)
	}
	if resp.PeerID == nil {
		t.Fatal("peer_id: want hostA, got nil")
	}
	if peer.ID(*resp.PeerID) != hostA {
		t.Fatalf("peer_id: want %s, got %s", hostA, resp.PeerID)
	}
}

func TestAuth_WrongPassword(t *testing.T) {
	a := newAuth(t)
	hostA, clientB := newPeer(t), newPeer(t)
	a.handleStream(hostA, fakeStream{
		Reader: writeReq(t, wire.InitRequest{Room: "r", Password: "pw"}),
		Writer: &bytes.Buffer{},
	})
	out := &bytes.Buffer{}
	a.handleStream(clientB, fakeStream{
		Reader: writeReq(t, wire.InitRequest{Room: "r", Password: "wrong"}),
		Writer: out,
	})
	resp := readResp(t, out)
	if resp.Status != wire.StatusErr {
		t.Fatalf("status: want Err, got %s", resp.Status)
	}
	if resp.PeerID != nil {
		t.Fatal("peer_id: want nil on auth failure")
	}
}

func TestAuth_HostReconnect(t *testing.T) {
	a := newAuth(t)
	hostA := newPeer(t)
	a.handleStream(hostA, fakeStream{
		Reader: writeReq(t, wire.InitRequest{Room: "r", Password: "pw"}),
		Writer: &bytes.Buffer{},
	})
	out := &bytes.Buffer{}
	a.handleStream(hostA, fakeStream{
		Reader: writeReq(t, wire.InitRequest{Room: "r", Password: "pw"}),
		Writer: out,
	})
	resp := readResp(t, out)
	if resp.Status != wire.StatusOk {
		t.Fatalf("status: want Ok, got %s", resp.Status)
	}
	if resp.PeerID != nil {
		t.Fatalf("peer_id: want nil for host reconnect, got %s", resp.PeerID)
	}
}

func TestAuth_TransferByHost(t *testing.T) {
	a := newAuth(t)
	hostA, hostB, clientC := newPeer(t), newPeer(t), newPeer(t)
	a.handleStream(hostA, fakeStream{
		Reader: writeReq(t, wire.InitRequest{Room: "r", Password: "pw"}),
		Writer: &bytes.Buffer{},
	})
	newHost := wire.PeerID(hostB)
	out := &bytes.Buffer{}
	a.handleStream(hostA, fakeStream{
		Reader: writeReq(t, wire.InitRequest{Room: "r", Password: "pw", TransferTo: &newHost}),
		Writer: out,
	})
	resp := readResp(t, out)
	if resp.Status != wire.StatusOk {
		t.Fatalf("status: want Ok, got %s", resp.Status)
	}
	if resp.PeerID != nil {
		t.Fatalf("peer_id: want nil on transfer, got %s", resp.PeerID)
	}
	out2 := &bytes.Buffer{}
	a.handleStream(clientC, fakeStream{
		Reader: writeReq(t, wire.InitRequest{Room: "r", Password: "pw"}),
		Writer: out2,
	})
	resp2 := readResp(t, out2)
	if peer.ID(*resp2.PeerID) != hostB {
		t.Fatalf("post-transfer host: want %s, got %s", hostB, resp2.PeerID)
	}
}

func TestAuth_TransferByNonHostSilentlyNoOp(t *testing.T) {
	// Matches the Rust quirk: unauthorised transfers respond Ok and do nothing.
	a := newAuth(t)
	hostA, attacker, clientX, clientC := newPeer(t), newPeer(t), newPeer(t), newPeer(t)
	a.handleStream(hostA, fakeStream{
		Reader: writeReq(t, wire.InitRequest{Room: "r", Password: "pw"}),
		Writer: &bytes.Buffer{},
	})
	newHost := wire.PeerID(attacker)
	out := &bytes.Buffer{}
	a.handleStream(clientX, fakeStream{
		Reader: writeReq(t, wire.InitRequest{Room: "r", Password: "pw", TransferTo: &newHost}),
		Writer: out,
	})
	resp := readResp(t, out)
	if resp.Status != wire.StatusOk {
		t.Fatalf("status: want Ok (silent no-op), got %s", resp.Status)
	}
	out2 := &bytes.Buffer{}
	a.handleStream(clientC, fakeStream{
		Reader: writeReq(t, wire.InitRequest{Room: "r", Password: "pw"}),
		Writer: out2,
	})
	resp2 := readResp(t, out2)
	if peer.ID(*resp2.PeerID) != hostA {
		t.Fatalf("host should be unchanged: want %s, got %s", hostA, resp2.PeerID)
	}
}

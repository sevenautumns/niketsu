package wire

import (
	"bytes"
	"encoding/hex"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/libp2p/go-libp2p/core/peer"
)

func loadHex(t *testing.T, name string) []byte {
	t.Helper()
	raw, err := os.ReadFile(filepath.Join("testdata", "golden", name))
	if err != nil {
		t.Fatalf("read %s: %v", name, err)
	}
	clean := strings.TrimSpace(string(raw))
	b, err := hex.DecodeString(clean)
	if err != nil {
		t.Fatalf("decode hex %s: %v", name, err)
	}
	return b
}

func TestGolden_InitRequestJoin_Decodes(t *testing.T) {
	want := InitRequest{Room: "movie-night", Password: "deadbeef"}
	dm, _ := DecMode()
	var got InitRequest
	if err := dm.Unmarshal(loadHex(t, "init_request_join.hex"), &got); err != nil {
		t.Fatalf("decode: %v", err)
	}
	if got.Room != want.Room || got.Password != want.Password || got.TransferTo != nil {
		t.Fatalf("mismatch: got %#v", got)
	}
}

func TestGolden_InitRequestTransfer_Decodes(t *testing.T) {
	dm, _ := DecMode()
	var got InitRequest
	if err := dm.Unmarshal(loadHex(t, "init_request_transfer.hex"), &got); err != nil {
		t.Fatalf("decode: %v", err)
	}
	if got.TransferTo == nil {
		t.Fatal("transfer_to should be set")
	}
	wantPid, _ := peer.Decode(fixturePeerB58)
	if peer.ID(*got.TransferTo) != wantPid {
		t.Fatalf("transfer_to mismatch: got %s want %s", got.TransferTo, wantPid)
	}
}

func TestGolden_InitResponseOk_RoundTrips(t *testing.T) {
	want := loadHex(t, "init_response_ok.hex")
	em, _ := EncMode()
	got, err := em.Marshal(InitResponse{Status: StatusOk})
	if err != nil {
		t.Fatal(err)
	}
	if !bytes.Equal(got, want) {
		t.Fatalf("byte mismatch:\nwant %x\ngot  %x", want, got)
	}
}

func TestGolden_InitResponseErr_RoundTrips(t *testing.T) {
	want := loadHex(t, "init_response_err.hex")
	em, _ := EncMode()
	got, err := em.Marshal(InitResponse{Status: StatusErr})
	if err != nil {
		t.Fatal(err)
	}
	if !bytes.Equal(got, want) {
		t.Fatalf("byte mismatch:\nwant %x\ngot  %x", want, got)
	}
}

func TestGolden_InitResponseOkWithPeer_RoundTrips(t *testing.T) {
	want := loadHex(t, "init_response_ok_with_peer.hex")
	pid, _ := peer.Decode(fixturePeerB58)
	pw := PeerID(pid)
	em, _ := EncMode()
	got, err := em.Marshal(InitResponse{Status: StatusOk, PeerID: &pw})
	if err != nil {
		t.Fatal(err)
	}
	if !bytes.Equal(got, want) {
		t.Fatalf("byte mismatch:\nwant %x\ngot  %x", want, got)
	}
}

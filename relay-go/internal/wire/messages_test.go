package wire

import (
	"testing"

	"github.com/fxamacker/cbor/v2"
	"github.com/libp2p/go-libp2p/core/peer"
)

func TestInitRequest_OmitsTransferToWhenNil(t *testing.T) {
	em, err := EncMode()
	if err != nil {
		t.Fatal(err)
	}
	req := InitRequest{Room: "movie-night", Password: "deadbeef"}
	out, err := em.Marshal(req)
	if err != nil {
		t.Fatal(err)
	}
	var m map[string]any
	if err := cbor.Unmarshal(out, &m); err != nil {
		t.Fatal(err)
	}
	if _, ok := m["transfer_to"]; ok {
		t.Fatalf("transfer_to should be absent when nil, got map: %#v", m)
	}
	if m["room"].(string) != "movie-night" {
		t.Fatalf("room mismatch: %#v", m["room"])
	}
}

func TestInitRequest_IncludesTransferToWhenSet(t *testing.T) {
	em, _ := EncMode()
	pid, _ := peer.Decode(fixturePeerB58)
	pw := PeerID(pid)
	req := InitRequest{Room: "r", Password: "pw", TransferTo: &pw}
	out, _ := em.Marshal(req)
	var m map[string]any
	cbor.Unmarshal(out, &m)
	if _, ok := m["transfer_to"]; !ok {
		t.Fatalf("transfer_to should be present, got: %#v", m)
	}
}

func TestInitResponse_StatusIsTextString(t *testing.T) {
	em, _ := EncMode()
	resp := InitResponse{Status: StatusOk}
	out, _ := em.Marshal(resp)
	var m map[string]any
	cbor.Unmarshal(out, &m)
	s, ok := m["status"].(string)
	if !ok || s != "Ok" {
		t.Fatalf("status should be CBOR text 'Ok', got %#v", m["status"])
	}
	if m["peer_id"] != nil {
		t.Fatalf("peer_id should be CBOR null, got %#v", m["peer_id"])
	}
}

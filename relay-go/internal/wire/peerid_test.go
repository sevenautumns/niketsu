package wire

import (
	"bytes"
	"testing"

	"github.com/fxamacker/cbor/v2"
	"github.com/libp2p/go-libp2p/core/peer"
)

// A known Ed25519-derived peer ID, in base58.
const fixturePeerB58 = "12D3KooWBhAY8GMTNyXfsiP9k3yQYUg3yj4KaUaG4nXjyu5gn2bN"

func TestPeerIDCBOR_MarshalAsByteString(t *testing.T) {
	pid, err := peer.Decode(fixturePeerB58)
	if err != nil {
		t.Fatal(err)
	}
	w := PeerID(pid)
	out, err := cbor.Marshal(&w)
	if err != nil {
		t.Fatalf("marshal: %v", err)
	}
	// The first CBOR major type for a byte string is 0x40-0x5F.
	if out[0]&0xE0 != 0x40 {
		t.Fatalf("expected CBOR byte string major type, got first byte 0x%02x", out[0])
	}
	// The byte-string contents must equal peer.MarshalBinary().
	raw, _ := pid.MarshalBinary()
	var rt []byte
	if err := cbor.Unmarshal(out, &rt); err != nil {
		t.Fatalf("decode generic: %v", err)
	}
	if !bytes.Equal(rt, raw) {
		t.Fatalf("byte-string contents differ:\nwant %x\ngot  %x", raw, rt)
	}
}

func TestPeerIDCBOR_RoundTrip(t *testing.T) {
	pid, _ := peer.Decode(fixturePeerB58)
	w := PeerID(pid)
	out, _ := cbor.Marshal(&w)
	var rt PeerID
	if err := cbor.Unmarshal(out, &rt); err != nil {
		t.Fatalf("unmarshal: %v", err)
	}
	if peer.ID(rt) != pid {
		t.Fatalf("round-trip differs: want %s got %s", pid, peer.ID(rt))
	}
}

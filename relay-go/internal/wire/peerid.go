// Package wire implements the /authorisation/1 request-response framing and
// message types used by the niketsu relay protocol. The wire format is a
// uvarint-length-prefixed CBOR encoding, byte-compatible with the Rust
// libp2p-rust `request_response::cbor::Behaviour`.
package wire

import (
	"errors"

	"github.com/fxamacker/cbor/v2"
	"github.com/libp2p/go-libp2p/core/peer"
)

// PeerID is a CBOR-encodable alias for peer.ID. It marshals as a CBOR byte
// string of peer.ID.MarshalBinary() to match the Rust libp2p serde impl
// (which serialises PeerId as `to_bytes()`).
type PeerID peer.ID

// MarshalCBOR encodes the peer ID as a CBOR byte string.
func (p *PeerID) MarshalCBOR() ([]byte, error) {
	if p == nil {
		return cbor.Marshal(nil)
	}
	raw, err := peer.ID(*p).MarshalBinary()
	if err != nil {
		return nil, err
	}
	return cbor.Marshal(raw)
}

// UnmarshalCBOR decodes a CBOR byte string (or null) into the peer ID.
func (p *PeerID) UnmarshalCBOR(data []byte) error {
	if p == nil {
		return errors.New("nil PeerID receiver")
	}
	// CBOR null is the single byte 0xF6.
	if len(data) == 1 && data[0] == 0xF6 {
		*p = PeerID("")
		return nil
	}
	var raw []byte
	if err := cbor.Unmarshal(data, &raw); err != nil {
		return err
	}
	pid, err := peer.IDFromBytes(raw)
	if err != nil {
		return err
	}
	*p = PeerID(pid)
	return nil
}

// String returns the base58 form of the peer ID for diagnostics.
func (p PeerID) String() string { return peer.ID(p).String() }

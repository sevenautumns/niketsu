package wire

import "github.com/fxamacker/cbor/v2"

// InitRequest is the client → relay payload on /authorisation/1.
//
// Wire shape:
//
//	{
//	  "room": <tstr>,
//	  "password": <tstr>,
//	  "transfer_to": <bstr>   ; OPTIONAL, omitted when nil
//	}
type InitRequest struct {
	Room       string  `cbor:"room"`
	Password   string  `cbor:"password"`
	TransferTo *PeerID `cbor:"transfer_to,omitempty"`
}

// Status is the response status. It is a CBOR text string on the wire,
// matching Rust serde unit-variant defaults.
type Status string

const (
	StatusOk             Status = "Ok"
	StatusErr            Status = "Err"
	StatusNotProvidingEr Status = "NotProvidingErr" // not emitted by relay; accepted from peers
)

// InitResponse is the relay → client payload on /authorisation/1.
//
// Wire shape:
//
//	{
//	  "status": "Ok" | "Err" | "NotProvidingErr",
//	  "peer_id": <bstr> | null
//	}
type InitResponse struct {
	Status Status  `cbor:"status"`
	PeerID *PeerID `cbor:"peer_id"`
}

// EncMode returns a CBOR encoder that emits struct fields in source order
// (matching Rust's serde-cbor / cbor4ii default) so that field ordering
// matches the Rust relay byte-for-byte.
func EncMode() (cbor.EncMode, error) {
	return cbor.EncOptions{Sort: cbor.SortNone}.EncMode()
}

// DecMode returns a CBOR decoder. The default tolerates extra and missing
// fields and accepts CBOR null for pointer fields.
func DecMode() (cbor.DecMode, error) {
	return cbor.DecOptions{}.DecMode()
}

package wire

import (
	"bufio"
	"fmt"
	"io"

	"github.com/multiformats/go-varint"
)

// MaxFrameBytes caps each CBOR payload to defend against an oversize length
// prefix from a misbehaving peer. The largest legitimate /authorisation/1
// message contains a room name, hex SHA-256, and an optional 64-byte peer
// id; ~1 KiB is plenty. We pick 16 MiB as a generous safety margin.
const MaxFrameBytes = 16 << 20

// readFrame reads one uvarint-length-prefixed payload from r.
func readFrame(r io.Reader) ([]byte, error) {
	br, ok := r.(io.ByteReader)
	if !ok {
		bufR := bufio.NewReader(r)
		br = bufR
		r = bufR
	}
	n, err := varint.ReadUvarint(br)
	if err != nil {
		return nil, fmt.Errorf("read length: %w", err)
	}
	if n > MaxFrameBytes {
		return nil, fmt.Errorf("frame length %d exceeds max %d", n, MaxFrameBytes)
	}
	buf := make([]byte, n)
	if _, err := io.ReadFull(r, buf); err != nil {
		return nil, fmt.Errorf("read payload: %w", err)
	}
	return buf, nil
}

// writeFrame writes a uvarint length prefix followed by the payload to w.
func writeFrame(w io.Writer, payload []byte) error {
	hdr := varint.ToUvarint(uint64(len(payload)))
	if _, err := w.Write(hdr); err != nil {
		return err
	}
	_, err := w.Write(payload)
	return err
}

// WriteInitRequest encodes req and writes it as one framed CBOR message.
func WriteInitRequest(w io.Writer, req InitRequest) error {
	em, err := EncMode()
	if err != nil {
		return err
	}
	payload, err := em.Marshal(req)
	if err != nil {
		return err
	}
	return writeFrame(w, payload)
}

// ReadInitRequest reads one framed CBOR message and decodes it as an InitRequest.
func ReadInitRequest(r io.Reader) (InitRequest, error) {
	payload, err := readFrame(r)
	if err != nil {
		return InitRequest{}, err
	}
	dm, err := DecMode()
	if err != nil {
		return InitRequest{}, err
	}
	var req InitRequest
	if err := dm.Unmarshal(payload, &req); err != nil {
		return InitRequest{}, fmt.Errorf("decode InitRequest: %w", err)
	}
	return req, nil
}

// WriteInitResponse encodes resp and writes it as one framed CBOR message.
func WriteInitResponse(w io.Writer, resp InitResponse) error {
	em, err := EncMode()
	if err != nil {
		return err
	}
	payload, err := em.Marshal(resp)
	if err != nil {
		return err
	}
	return writeFrame(w, payload)
}

// ReadInitResponse reads one framed CBOR message and decodes it as an InitResponse.
func ReadInitResponse(r io.Reader) (InitResponse, error) {
	payload, err := readFrame(r)
	if err != nil {
		return InitResponse{}, err
	}
	dm, err := DecMode()
	if err != nil {
		return InitResponse{}, err
	}
	var resp InitResponse
	if err := dm.Unmarshal(payload, &resp); err != nil {
		return InitResponse{}, fmt.Errorf("decode InitResponse: %w", err)
	}
	return resp, nil
}

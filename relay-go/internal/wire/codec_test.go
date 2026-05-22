package wire

import (
	"bytes"
	"io"
	"strings"
	"testing"
)

func TestRoundTripInitRequest(t *testing.T) {
	want := InitRequest{Room: "r1", Password: "deadbeef"}
	var buf bytes.Buffer
	if err := WriteInitRequest(&buf, want); err != nil {
		t.Fatal(err)
	}
	got, err := ReadInitRequest(&buf)
	if err != nil {
		t.Fatal(err)
	}
	if got.Room != want.Room || got.Password != want.Password || got.TransferTo != nil {
		t.Fatalf("round-trip differs: got %#v", got)
	}
}

func TestRoundTripInitResponse(t *testing.T) {
	want := InitResponse{Status: StatusErr}
	var buf bytes.Buffer
	if err := WriteInitResponse(&buf, want); err != nil {
		t.Fatal(err)
	}
	got, err := ReadInitResponse(&buf)
	if err != nil {
		t.Fatal(err)
	}
	if got.Status != StatusErr || got.PeerID != nil {
		t.Fatalf("round-trip differs: got %#v", got)
	}
}

func TestReadInitRequest_TruncatedFrame(t *testing.T) {
	// Length prefix says 100 bytes, but only 5 follow.
	buf := bytes.NewBuffer([]byte{0x64 /* uvarint 100 */, 'a', 'b', 'c', 'd', 'e'})
	_, err := ReadInitRequest(buf)
	if err == nil || err == io.EOF {
		t.Fatalf("expected non-EOF error for truncated frame, got %v", err)
	}
}

func TestReadInitRequest_LengthTooLarge(t *testing.T) {
	// uvarint > MaxFrameBytes must be rejected before allocating the payload.
	huge := []byte{}
	const tooBig = uint64(16<<20) + 1
	for v := tooBig; ; {
		if v < 0x80 {
			huge = append(huge, byte(v))
			break
		}
		huge = append(huge, byte(v)|0x80)
		v >>= 7
	}
	huge = append(huge, bytes.Repeat([]byte{0}, 4)...)
	_, err := ReadInitRequest(strings.NewReader(string(huge)))
	if err == nil {
		t.Fatal("expected error for oversize length prefix")
	}
}

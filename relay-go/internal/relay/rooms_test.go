package relay

import (
	"sync"
	"testing"
	"time"

	"github.com/libp2p/go-libp2p/core/peer"
)

const (
	hostA = peer.ID("\x00\x24\x08\x01\x12\x20" + "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA")
	hostB = peer.ID("\x00\x24\x08\x01\x12\x20" + "BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB")
)

func TestRooms_CreateOnFirstJoin(t *testing.T) {
	rs := newRooms()
	res, err := rs.Join(hostA, "room1", "hashedpw")
	if err != nil {
		t.Fatal(err)
	}
	if !res.Created {
		t.Fatal("expected Created=true on first join")
	}
	if res.Host != hostA {
		t.Fatalf("Host: want %s got %s", hostA, res.Host)
	}
}

func TestRooms_VerifyExistingMatchingPassword(t *testing.T) {
	rs := newRooms()
	rs.Join(hostA, "room1", "hashedpw")
	res, err := rs.Join(hostB, "room1", "hashedpw")
	if err != nil {
		t.Fatal(err)
	}
	if res.Created {
		t.Fatal("expected Created=false on join into existing room")
	}
	if res.Host != hostA {
		t.Fatalf("Host: want %s got %s", hostA, res.Host)
	}
}

func TestRooms_WrongPassword(t *testing.T) {
	rs := newRooms()
	rs.Join(hostA, "room1", "hashedpw")
	_, err := rs.Join(hostB, "room1", "wrong")
	if err != ErrWrongPassword {
		t.Fatalf("expected ErrWrongPassword, got %v", err)
	}
}

func TestRooms_TransferOnlyByCurrentHost(t *testing.T) {
	rs := newRooms()
	rs.Join(hostA, "room1", "pw")
	if err := rs.Transfer(hostB, "room1", hostA); err != ErrNotHost {
		t.Fatalf("transfer by non-host: want ErrNotHost, got %v", err)
	}
	if err := rs.Transfer(hostA, "room1", hostB); err != nil {
		t.Fatalf("transfer by host: want nil, got %v", err)
	}
	res, _ := rs.Join(hostA, "room1", "pw")
	if res.Host != hostB {
		t.Fatalf("after transfer, host should be %s, got %s", hostB, res.Host)
	}
}

func TestRooms_DisconnectRemovesRoomIfHost(t *testing.T) {
	rs := newRooms()
	rs.Join(hostA, "room1", "pw")
	rs.OnDisconnect(hostA)
	res, err := rs.Join(hostB, "room1", "pw")
	if err != nil {
		t.Fatal(err)
	}
	if !res.Created || res.Host != hostB {
		t.Fatal("after disconnect, room should be recreatable by new peer")
	}
}

func TestRooms_DisconnectIgnoresNonHost(t *testing.T) {
	rs := newRooms()
	rs.Join(hostA, "room1", "pw")
	rs.OnDisconnect(hostB)
	res, _ := rs.Join(hostB, "room1", "pw")
	if res.Created {
		t.Fatal("room should still exist after non-host disconnect")
	}
}

func TestRooms_ConcurrentVerifiesDontSerialise(t *testing.T) {
	rs := newRooms()
	const N = 4
	for i := 0; i < N; i++ {
		roomName := "room" + string(rune('A'+i))
		host := peer.ID("\x00\x24\x08\x01\x12\x20" + string(rune('A'+i)) + "1234567890123456789012345678901")
		_, err := rs.Join(host, roomName, "pw")
		if err != nil {
			t.Fatal(err)
		}
	}

	// Measure single bcrypt verify time (warm cost).
	start := time.Now()
	_, _ = rs.Join(peer.ID("client"), "roomA", "pw")
	single := time.Since(start)

	// N concurrent verifies on different rooms. If bcrypt runs under a
	// shared lock, total time will be ~N*single. If it runs in parallel,
	// total time will be ~single + scheduler overhead.
	start = time.Now()
	var wg sync.WaitGroup
	for i := 0; i < N; i++ {
		wg.Add(1)
		go func(i int) {
			defer wg.Done()
			roomName := "room" + string(rune('A'+i))
			_, _ = rs.Join(peer.ID("client"), roomName, "pw")
		}(i)
	}
	wg.Wait()
	total := time.Since(start)

	maxAllowed := time.Duration(float64(single*N) * 0.7)
	if total > maxAllowed {
		t.Fatalf("verifies appear serialised: %d concurrent took %v, single %v, max allowed %v", N, total, single, maxAllowed)
	}
}

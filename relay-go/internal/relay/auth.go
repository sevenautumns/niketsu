package relay

import (
	"errors"
	"io"
	"log/slog"

	"github.com/libp2p/go-libp2p/core/peer"
	"github.com/sevenautumns/niketsu/relay-go/internal/wire"
)

// authService implements the /authorisation/1 protocol: read one InitRequest,
// run the state machine, write one InitResponse.
//
// It depends only on `rooms` and an optional `verifyCache`; no libp2p types.
// The libp2p stream is plumbed in as an io.ReadWriter by the relay layer.
type authService struct {
	rooms   *rooms
	cache   *verifyCache
	metrics *metrics
}

// handleStream processes one auth exchange on rw, attributing it to caller.
// Errors are logged but not surfaced — there is no protocol-level error frame.
func (a *authService) handleStream(caller peer.ID, rw io.ReadWriter) {
	req, err := wire.ReadInitRequest(rw)
	if err != nil {
		slog.Debug("auth: read request failed", "peer", caller, "err", err)
		return
	}

	resp := a.dispatch(caller, req)

	if err := wire.WriteInitResponse(rw, resp); err != nil {
		slog.Debug("auth: write response failed", "peer", caller, "err", err)
		return
	}
}

func (a *authService) dispatch(caller peer.ID, req wire.InitRequest) wire.InitResponse {
	if req.TransferTo != nil {
		return a.handleTransfer(caller, req)
	}
	return a.handleJoin(caller, req)
}

func (a *authService) handleTransfer(caller peer.ID, req wire.InitRequest) wire.InitResponse {
	newHost := peer.ID(*req.TransferTo)
	if err := a.rooms.Transfer(caller, req.Room, newHost); err != nil {
		// Matches Rust: silent no-op on unauthorised transfer.
		slog.Debug("auth: transfer rejected", "peer", caller, "room", req.Room, "err", err)
	}
	if a.metrics != nil {
		a.metrics.AuthRequest("transfer")
	}
	return wire.InitResponse{Status: wire.StatusOk}
}

func (a *authService) handleJoin(caller peer.ID, req wire.InitRequest) wire.InitResponse {
	if a.cache != nil && a.cache.Hit(caller, req.Room, req.Password) {
		host, ok := a.rooms.LookupHost(req.Room)
		if ok {
			if a.metrics != nil {
				a.metrics.VerifyCache("hit")
				a.metrics.AuthRequest("ok")
			}
			if host == caller {
				return wire.InitResponse{Status: wire.StatusOk}
			}
			pw := wire.PeerID(host)
			return wire.InitResponse{Status: wire.StatusOk, PeerID: &pw}
		}
		// Cache referenced a room that has since vanished — fall through.
	}

	res, err := a.rooms.Join(caller, req.Room, req.Password)
	if a.metrics != nil {
		a.metrics.VerifyCache("miss")
	}
	switch {
	case errors.Is(err, ErrWrongPassword):
		if a.metrics != nil {
			a.metrics.AuthRequest("err")
		}
		return wire.InitResponse{Status: wire.StatusErr}
	case err != nil:
		slog.Debug("auth: join failed", "peer", caller, "room", req.Room, "err", err)
		if a.metrics != nil {
			a.metrics.AuthRequest("err")
		}
		return wire.InitResponse{Status: wire.StatusErr}
	}

	if a.cache != nil {
		a.cache.Store(caller, req.Room, req.Password)
	}
	if a.metrics != nil {
		a.metrics.AuthRequest("ok")
	}
	if res.Created || res.Host == caller {
		return wire.InitResponse{Status: wire.StatusOk}
	}
	pw := wire.PeerID(res.Host)
	return wire.InitResponse{Status: wire.StatusOk, PeerID: &pw}
}

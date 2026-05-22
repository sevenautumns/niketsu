package relay

import (
	"github.com/libp2p/go-libp2p/core/network"
	ma "github.com/multiformats/go-multiaddr"
)

// disconnectNotifiee removes a peer's room when their last connection closes.
type disconnectNotifiee struct {
	rooms *rooms
}

func (d *disconnectNotifiee) Listen(network.Network, ma.Multiaddr)      {}
func (d *disconnectNotifiee) ListenClose(network.Network, ma.Multiaddr) {}
func (d *disconnectNotifiee) Connected(network.Network, network.Conn)   {}
func (d *disconnectNotifiee) Disconnected(_ network.Network, c network.Conn) {
	// Disconnected fires when the LAST connection to a peer is torn down.
	// This matches the Rust relay's `SwarmEvent::ConnectionClosed` semantics.
	d.rooms.OnDisconnect(c.RemotePeer())
}

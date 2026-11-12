//! End to end over the real network stack: every scenario starts a relay on
//! a loopback port and several cores, each running its real event loop with
//! the real [`niketsu_communicator::P2PCommunicator`]. Only the edges a
//! person would touch are fakes: the player and the UI.
//!
//! Where the `dance` in `niketsu-core` specifies the protocol with an
//! in-memory network, these scenarios check that the libp2p transport
//! carries it: room authorisation at the relay, the direct connection
//! between peers, gossip, direct requests and the host handover.
//!
//! Nothing here is instant, so scenarios state what must *eventually* hold
//! (see `harness::eventually`) and never sleep for a fixed time.

mod harness;
mod scenarios;

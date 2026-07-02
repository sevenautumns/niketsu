//! The dance: an executable specification of the multi-client protocol.
//!
//! Each scenario wires several `CoreModel`s together through an in-memory
//! [`harness::Room`] that plays the relay: every peer's `OutgoingMessage`s
//! are routed to the other peers as `IncomingMessage`s following the real
//! relay's rules (see `Room::route`). Subsystems are stateful fakes, so
//! scenarios assert *state* ("bob is paused at 0s"), not call sequences.
//!
//! Heartbeats and player time are explicit — nothing here is timing
//! dependent. `Room::pump` delivers messages until the room is quiescent
//! and panics on a message storm, so every scenario also checks the
//! no-echo rule: handling a broadcast must never re-emit that broadcast.
//!
//! Simplification versus the real network: zero latency, so the
//! client-side half-RTT position compensation does not apply.

mod harness;
mod scenarios;

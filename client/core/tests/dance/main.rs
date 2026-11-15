//! The dance: an executable specification of the multi-client protocol.
//!
//! Each scenario wires several `CoreModel`s together through an in-memory
//! [`harness::Room`] that plays the network: every peer's
//! `OutgoingMessage`s are routed to the other peers as `IncomingMessage`s
//! following the communicator's transport rules (see `Room::route`). The
//! room policy itself — user list, renames, auto-start, what late joiners
//! get — runs in the host's core. Subsystems are stateful fakes, so
//! scenarios assert *state* ("bob is paused at 0s"), not call sequences.
//!
//! Heartbeats and player time are explicit — nothing here is timing
//! dependent. `Room::pump` delivers messages until the room is quiescent
//! and panics on a message storm, so every scenario also checks the
//! no-echo rule: handling a broadcast must never re-emit that broadcast.
//!
//! Delivery is instant; round trips to the host exist only where a
//! scenario injects them with `Room::round_trip`. What the host sends a
//! member goes through the member's outbox as the communicator keeps it:
//! in order, and held back only where a scenario holds the link with
//! `Room::hold_link`.

mod harness;
mod properties;
mod scenarios;

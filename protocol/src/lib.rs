//! What a peer and the relay say to each other about rooms. Both sides
//! build against these types, so the wire format cannot differ between them.

use libp2p::PeerId;
use serde::{Deserialize, Serialize};

/// The request-response protocol the messages below travel on.
pub const AUTH_PROTOCOL: &str = "/authorisation/1";

/// A peer asks for a room: to join it, to open it if nobody hosts it, or,
/// as its host, to hand it over.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthRequest {
    pub room: String,
    /// A digest of the room's password, never the password itself.
    pub password: String,
    /// The peer the room's host hands the room over to.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub transfer_to: Option<PeerId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthResponse {
    pub status: AuthStatus,
    /// The room's host, unless the asking peer hosts the room itself.
    pub peer_id: Option<PeerId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AuthStatus {
    Ok,
    Err,
}

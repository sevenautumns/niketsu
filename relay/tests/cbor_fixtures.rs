//! Prints CBOR hex for canonical InitRequest / InitResponse values. Run with
//!     cargo test --package niketsu-relay --test cbor_fixtures -- --nocapture print_fixtures
//! and copy the printed hex into relay-go/internal/wire/testdata/golden/.

use std::str::FromStr;

use libp2p::PeerId;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct InitRequest {
    room: String,
    password: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    transfer_to: Option<PeerId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
enum Status {
    Ok,
    Err,
    NotProvidingErr,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct InitResponse {
    status: Status,
    peer_id: Option<PeerId>,
}

fn dump<T: Serialize>(name: &str, v: &T) {
    let out: Vec<u8> = cbor4ii::serde::to_vec(Vec::new(), v).unwrap();
    println!("=== {name} ===");
    for b in &out {
        print!("{b:02x}");
    }
    println!();
}

#[test]
fn print_fixtures() {
    let fixture_peer = PeerId::from_str("12D3KooWBhAY8GMTNyXfsiP9k3yQYUg3yj4KaUaG4nXjyu5gn2bN").unwrap();
    dump("init_request_join", &InitRequest {
        room: "movie-night".to_string(),
        password: "deadbeef".to_string(),
        transfer_to: None,
    });
    dump("init_request_transfer", &InitRequest {
        room: "movie-night".to_string(),
        password: "deadbeef".to_string(),
        transfer_to: Some(fixture_peer),
    });
    dump("init_response_ok", &InitResponse {
        status: Status::Ok,
        peer_id: None,
    });
    dump("init_response_err", &InitResponse {
        status: Status::Err,
        peer_id: None,
    });
    dump("init_response_ok_with_peer", &InitResponse {
        status: Status::Ok,
        peer_id: Some(fixture_peer),
    });
}

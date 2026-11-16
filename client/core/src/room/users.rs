//! How the room's authority names its users: every user goes by a name no
//! other user of the room goes by.

use std::collections::BTreeMap;

use arcstr::ArcStr;
use fake::Fake;
use fake::faker::company::en::Buzzword;
use multiaddr::PeerId;

use crate::user::User;

/// Where the suffix of a given name comes from: a user whose name is taken
/// goes by `name_<suffix>`.
pub type NameSuffixes = fn() -> String;

pub(super) fn buzzword() -> String {
    Buzzword().fake()
}

/// How many suffixes are tried before a counter is appended, so finding a
/// free name ends even when every suffix is taken.
const SUFFIX_ROLLS: usize = 16;

/// Whether a user other than the one at `peer` goes by `name`.
fn taken(users: &BTreeMap<PeerId, User>, name: &str, peer: PeerId) -> bool {
    users
        .iter()
        .any(|(other, user)| *other != peer && user.name == name)
}

/// The name the user at `peer` goes by when it asks for `name`: that name
/// if no other user goes by it, otherwise `name_<suffix>`, rolled again
/// while taken, with a counter appended once the rolls are used up.
pub(super) fn free_name(
    users: &BTreeMap<PeerId, User>,
    name: &ArcStr,
    peer: PeerId,
    suffixes: NameSuffixes,
) -> ArcStr {
    if !taken(users, name, peer) {
        return name.clone();
    }
    let mut candidate = ArcStr::default();
    for _ in 0..SUFFIX_ROLLS {
        candidate = arcstr::format!("{name}_{}", suffixes());
        if !taken(users, &candidate, peer) {
            return candidate;
        }
    }
    let mut counter = 2u32;
    loop {
        let numbered = arcstr::format!("{candidate}_{counter}");
        if !taken(users, &numbered, peer) {
            return numbered;
        }
        counter += 1;
    }
}

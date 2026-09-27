//! Negative: a provider crate that lists a reply it does not define fails
//! in its own build, before any firmware assembles it.

use ankyra_macros::ankyra_provider;

ankyra_provider! {
    name: TYPO,
    replies: [crate::replies::Missing],
}

mod replies {}

fn main() {}

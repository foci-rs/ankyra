use ankyra::klipper_static_string;

// No `ankyra_config!` invocation in this crate — so the
// `crate::_ankyra_config::static_strings::__ANKYRA_SS_<hash>` path the
// macro emits cannot resolve.
fn main() {
    let _id: u16 = klipper_static_string!("orphan");
}

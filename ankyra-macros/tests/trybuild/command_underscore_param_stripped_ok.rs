// Verify that a handler parameter named `_oid` (leading underscore to
// silence the `unused_variables` lint) emits `oid` in the Klipper wire
// format string, not `_oid`. Klipper's host compares format strings
// byte-for-byte against its own `DECL_COMMAND` shape and rejects a
// mismatch with `Command format mismatch` at identify-time — so the
// underscore must be stripped before the format string is baked into
// the data dictionary.
//
// The Rust-level binding still sees `_oid`; only the wire name is
// rewritten.
use ankyra_macros::klipper_command;

pub struct State;

#[klipper_command]
fn set_pin(_ctx: &mut State, _oid: u8, value: u8) {
    let _ = (_oid, value);
}

// A handler where the underscore *is* load-bearing (double-underscore
// and bare-underscore shapes must be preserved verbatim). `__oid` is
// reserved-ish — never rewritten.
#[klipper_command]
fn double_underscore(_ctx: &mut State, __oid: u8) {
    let _ = __oid;
}

fn main() {
    // The sibling `__ANKYRA_FORMAT_command_<name>` const is the
    // authoritative wire-format string the assembler splices into the
    // data dictionary via `concatcp!`.
    assert_eq!(__ANKYRA_FORMAT_command_set_pin, "set_pin oid=%c value=%c");
    assert_eq!(
        __ANKYRA_FORMAT_command_double_underscore,
        "double_underscore __oid=%c"
    );
    // The dispatch wrappers still compile against handlers that bind
    // the parameter as `_oid` / `__oid`.
    let _ = __ankyra_dispatch_set_pin::<()>;
    let _ = __ankyra_dispatch_double_underscore::<()>;
}

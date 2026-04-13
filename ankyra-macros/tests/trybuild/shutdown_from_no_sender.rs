fn main() {
    // Missing the mandatory sender expression and the comma that
    // separates it from the reason literal. `klipper_shutdown_from!`
    // requires `(sender_expr, "reason", clock_expr)`; invoking it with
    // just a bare literal triggers the parser's "no comma after first
    // expr" arm which emits a helpful usage message.
    ::ankyra::klipper_shutdown_from!("hardware fault");
}

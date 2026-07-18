#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Self-report the compiled-in release channel for the release smoke step,
    // before any GUI/event-loop init so it works headless in CI.
    if posthaste_client_desktop_lib::handle_print_release_channel() {
        return;
    }
    posthaste_client_desktop_lib::run();
}

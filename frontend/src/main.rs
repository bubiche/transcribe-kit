pub mod app;
mod features;
mod live_recording;
mod tauri_api;

fn main() {
    console_error_panic_hook::set_once();
    leptos::mount::mount_to_body(app::App);
}

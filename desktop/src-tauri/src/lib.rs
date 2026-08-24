/// CourseMaster's desktop shell is intentionally thin: all data lives behind
/// the hosted API (see `crates/api-server`), so this binary's only job is to
/// embed and display the same web UI a browser would load from the VPS. No
/// local database, no local AI provider, no custom commands — the frontend
/// talks to the hosted API directly over HTTPS via `ui/src/api.ts`.
pub fn run() {
    tracing_subscriber::fmt::try_init().ok();

    tauri::Builder::default()
        .run(tauri::generate_context!())
        .expect("error while running the CourseMaster application");
}

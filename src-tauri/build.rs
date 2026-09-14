fn main() {
    // Windows runs the whole app elevated: every operation it performs (writing
    // into ProgramData, registering and driving the service) needs administrator
    // rights, and there is no separate privilege boundary to fall back on.
    //
    // The manifest is attached to release builds only. Test harnesses are
    // separate executables built from this same crate, so an unconditional
    // `requireAdministrator` manifest would make `cargo test` unlaunchable from
    // an ordinary shell (Windows error 740). During development the app must be
    // started from an elevated terminal instead.
    #[cfg(windows)]
    if std::env::var("PROFILE").as_deref() == Ok("release") {
        let attributes = tauri_build::Attributes::new().windows_attributes(
            tauri_build::WindowsAttributes::new()
                .app_manifest(include_str!("windows-app.manifest")),
        );
        tauri_build::try_build(attributes).expect("failed to run tauri-build");
        return;
    }

    tauri_build::build();
}

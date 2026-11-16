fn main() {
    println!("cargo:rerun-if-changed=assets/activity-tracker.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    let mut resources = winresource::WindowsResource::new();
    // Resource ID 1: also used by GPUI and the classic window classes.
    resources.set_icon("assets/activity-tracker.ico");

    // GPUI embeds its own manifest (also asInvoker, plus per-monitor DPI
    // awareness and Common Controls v6). Only add ours for the classic build
    // to avoid a duplicate MANIFEST resource; the icon is always embedded.
    if std::env::var_os("CARGO_FEATURE_GUI_GPUI").is_none() {
        resources.set_manifest(
            r#"<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
    <security>
      <requestedPrivileges>
        <requestedExecutionLevel level="asInvoker" uiAccess="false" />
      </requestedPrivileges>
    </security>
  </trustInfo>
</assembly>"#,
        );
        // asInvoker keeps `cargo run` and double-clicks working; the app
        // elevates itself at runtime when the admin-only PawnIO device
        // (INF: `D:P(A;;GA;;;SY)(A;;GA;;;BA)`) denies access.
    }
    resources
        .compile_for("activity-tracker-win-rs")
        .expect("failed to compile Windows resources");
}

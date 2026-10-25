fn main() {
    // With the GPUI frontend, gpui embeds its own manifest (also asInvoker,
    // plus per-monitor DPI awareness and Common Controls v6): a second one
    // fails the link with a duplicate MANIFEST resource.
    let gpui = std::env::var_os("CARGO_FEATURE_GUI_GPUI").is_some();
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") && !gpui {
        winresource::WindowsResource::new()
            .set_manifest(
                r#"<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
    <security>
      <requestedPrivileges>
        <requestedExecutionLevel level="asInvoker" uiAccess="false" />
      </requestedPrivileges>
    </security>
  </trustInfo>
</assembly>"#,
            )
            // asInvoker keeps `cargo run` and double-clicks working; the app
            // elevates itself at runtime when the admin-only PawnIO device
            // (INF: `D:P(A;;GA;;;SY)(A;;GA;;;BA)`) denies access.
            .compile_for("activity-tracker-win-rs")
            .expect("failed to compile Windows resources");
    }
}

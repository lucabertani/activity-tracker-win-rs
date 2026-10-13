fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
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

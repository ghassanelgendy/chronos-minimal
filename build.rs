// Embed Windows manifest and app icon. Requires icon.ico in project root for tray/exe icon.

fn main() {
    #[cfg(windows)]
    {
        let mut res = winres::WindowsResource::new();
        res.set_manifest(
            r#"
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <dependency>
    <dependentAssembly>
      <assemblyIdentity type="win32" name="Microsoft.Windows.Common-Controls" version="6.0.0.0" processorArchitecture="*" publicKeyToken="6595b64144ccf1df" language="*"/>
    </dependentAssembly>
  </dependency>
</assembly>
"#,
        );
        if std::path::Path::new("icon.ico").exists() {
            res.set_icon("icon.ico");
        }
        if let Err(e) = res.compile() {
            eprintln!("winres: {}", e);
            std::process::exit(1);
        }
    }
}

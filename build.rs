fn main() {
    println!("cargo:rerun-if-changed=assets/tokens-and-code.ico");
    println!("cargo:rerun-if-changed=assets/tokens-and-code.png");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut resource = winresource::WindowsResource::new();
        resource.set_icon("assets/tokens-and-code.ico");
        resource.set("FileDescription", "Tokens and Code");
        resource.set("ProductName", "Tokens and Code");
        resource.set("OriginalFilename", "TokensAndCode.exe");
        resource
            .compile()
            .expect("failed to embed Windows app icon");
    }
}

fn main() {
    // Ícone e metadados do Conver.exe
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/conver.ico");
        res.set("ProductName", "Conver");
        res.set("FileDescription", "Conver - conversor universal");
        res.compile().expect("falha ao embutir o ícone");
    }
    println!("cargo:rerun-if-changed=assets/conver.ico");
}

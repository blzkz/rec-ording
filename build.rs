// Gives the Windows .exe its icon and name (Explorer, taskbar pins); nothing to do elsewhere.
fn main() {
    println!("cargo:rerun-if-changed=assets/icons/rec-ording.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let mut res = winresource::WindowsResource::new();
    res.set_icon("assets/icons/rec-ording.ico");
    res.set("ProductName", "rec·ording");
    res.set("FileDescription", "rec·ording");
    if let Err(e) = res.compile() {
        println!("cargo:warning=No se pudo añadir el icono al ejecutable: {e}");
    }
}

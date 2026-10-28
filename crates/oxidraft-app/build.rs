fn main() {
    // The updater picks the release asset built for the same target triple.
    let target = std::env::var("TARGET").expect("cargo sets TARGET for build scripts");
    println!("cargo:rustc-env=OXIDRAFT_TARGET={target}");

    // Give oxidraft.exe its icon (Explorer, taskbar, installer shortcuts).
    #[cfg(windows)]
    {
        println!("cargo:rerun-if-changed=../../assets/icon.ico");
        let mut res = winresource::WindowsResource::new();
        res.set_icon("../../assets/icon.ico")
            .set("ProductName", "Oxidraft")
            .set("FileDescription", "Oxidraft 2D drafting")
            .set("LegalCopyright", "The Oxidraft contributors, MIT OR Apache-2.0");
        res.compile().expect("embed the Windows icon");
    }
}

fn main() {
    println!("cargo:rerun-if-changed=native/client.c");
    println!("cargo:rerun-if-env-changed=OMARCHY_VNC_NATIVE_ROOT");
    let mut compiler = cc::Build::new();
    compiler.file("native/client.c").warnings(true);
    if let Some(root) = std::env::var_os("OMARCHY_VNC_NATIVE_ROOT") {
        let root = std::path::PathBuf::from(root);
        compiler.include(root.join("usr/include"));
        // Private development runtime; never modify the system library directories.
        let out = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
        let target = out.ancestors().nth(4).unwrap();
        let private = target.join("lib/omarchy-vnc");
        std::fs::create_dir_all(&private).unwrap();
        std::fs::copy(
            root.join("usr/lib/libvncclient.so.1"),
            private.join("libvncclient.so.1"),
        )
        .unwrap();
        println!(
            "cargo:rustc-link-search=native={}",
            root.join("usr/lib").display()
        );
        println!("cargo:rustc-link-lib=vncclient");
    } else {
        let library = pkg_config::Config::new()
            .atleast_version("0.9.15")
            .probe("libvncclient")
            .expect("Install libvncserver (Omarchy/Arch: omarchy pkg add libvncserver)");
        for path in library.include_paths {
            compiler.include(path);
        }
    }
    compiler.compile("omarchy_vnc_adapter");
}

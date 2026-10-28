fn main() {
    // The updater picks the release asset built for the same target triple.
    let target = std::env::var("TARGET").expect("cargo sets TARGET for build scripts");
    println!("cargo:rustc-env=OXIDRAFT_TARGET={target}");
}

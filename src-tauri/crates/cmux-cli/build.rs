//! Gives `mira` on Windows the 8 MB main-thread stack macOS and Linux give.
//!
//! MSVC links a 1 MB main stack by default, and a debug build of `mira`
//! overflows it just parsing its arguments: clap's tree of some thirty
//! subcommands is built on that stack, and Windows' debug frames are
//! larger than macOS's. Windows CI failed every completion test that way.
//! A release build fits, but with little to spare as subcommands grow.
//!
//! The target is read from the environment, not `cfg!`: a build script
//! runs on the host, which is not the target when cross-compiling.

fn main() {
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let target_env = std::env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    if target_os == "windows" && target_env == "msvc" {
        println!("cargo:rustc-link-arg-bins=/STACK:8388608");
    }
}

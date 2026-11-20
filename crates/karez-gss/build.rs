//! Links the system GSS-API library. Nothing is bundled and nothing is downloaded.
//!
//! * Linux and other ELF Unix: the MIT Kerberos runtime library, by its soname (`+verbatim`), so no
//!   development package and no `libgssapi_krb5.so` symlink is needed. `KAREZ_GSS_LIB=name` links
//!   `-lname` instead, for example `gssapi_krb5` or `gssapi`.
//! * macOS: the `GSS` framework.
//! * Windows: no GSS-API; SSPI is a different interface and is not implemented yet.

fn main() {
    println!("cargo:rerun-if-env-changed=KAREZ_GSS_LIB");
    println!("cargo:rerun-if-changed=build.rs");
    let os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if let Ok(name) = std::env::var("KAREZ_GSS_LIB") {
        println!("cargo:rustc-link-lib={name}");
        return;
    }
    match os.as_str() {
        "macos" => println!("cargo:rustc-link-lib=framework=GSS"),
        "windows" => {}
        _ => println!("cargo:rustc-link-lib=dylib:+verbatim=libgssapi_krb5.so.2"),
    }
}

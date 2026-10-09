fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=XSSC_SOURCE_REVISION");
    let revision = match std::env::var("XSSC_SOURCE_REVISION") {
        Ok(value) => {
            assert!(
                value.len() == 40
                    && value
                        .bytes()
                        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
                "formal source revision must be a full lowercase Git hash"
            );
            value
        }
        Err(std::env::VarError::NotPresent) => "unbound".to_owned(),
        Err(_) => panic!("source revision must be UTF-8"),
    };
    let target = std::env::var("TARGET").expect("Cargo target is required");
    println!("cargo:rustc-env=XSSC_COMPILED_SOURCE_REVISION={revision}");
    println!("cargo:rustc-env=XSSC_COMPILED_TARGET={target}");
}

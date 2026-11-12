fn main() {
    let manifest = std::env::var("CARGO_MANIFEST_DIR").expect("manifest");
    let out = std::env::var("OUT_DIR").expect("out");
    let target = std::env::var("TARGET").unwrap_or_default();
    let windows = target.contains("windows");
    let cc = if windows { "x86_64-w64-mingw32-gcc" } else { "cc" };
    let ar = if windows { "x86_64-w64-mingw32-ar" } else { "ar" };
    let source = format!("{manifest}/src/login_account.c");
    let object = format!("{out}/login_account.o");
    let archive = format!("{out}/liblogin_account.a");
    let mut compile = vec!["-c".to_string(), "-O2".to_string(), "-std=c11".to_string()];
    if !windows {
        compile.push("-fPIC".to_string());
    }
    compile.push(source);
    compile.push("-o".to_string());
    compile.push(object.clone());
    let compiled = std::process::Command::new(cc)
        .args(&compile)
        .status()
        .map(|status| status.success())
        .unwrap_or(false);
    if !compiled {
        panic!("login account compile failed");
    }
    let archived = std::process::Command::new(ar)
        .args(["rcs", &archive, &object])
        .status()
        .map(|status| status.success())
        .unwrap_or(false);
    if !archived {
        panic!("login account archive failed");
    }
    println!("cargo:rustc-link-search=native={out}");
    println!("cargo:rustc-link-lib=static=login_account");
    println!("cargo:rerun-if-changed=src/login_account.c");
}

use std::{env, path::PathBuf};
fn main() {
    println!("cargo:rustc-check-cfg=cfg(occt_available)");
    println!("cargo:rerun-if-changed=native/bridge.cpp");
    for key in ["OCCT_ROOT", "OCCT_INCLUDE_DIR", "OCCT_LIB_DIR"] {
        println!("cargo:rerun-if-env-changed={key}");
    }
    if env::var_os("CARGO_FEATURE_OCCT").is_none() {
        return;
    }
    let root = env::var_os("OCCT_ROOT").map(PathBuf::from);
    let include = env::var_os("OCCT_INCLUDE_DIR")
        .map(PathBuf::from)
        .or_else(|| root.as_ref().map(|p| p.join("include/opencascade")))
        .or_else(|| {
            [
                "/opt/homebrew/include/opencascade",
                "/usr/include/opencascade",
                "/usr/local/include/opencascade",
            ]
            .into_iter()
            .map(PathBuf::from)
            .find(|p| p.join("STEPControl_Reader.hxx").exists())
        });
    let Some(include) = include.filter(|p| p.join("STEPControl_Reader.hxx").exists()) else {
        panic!(
            "The occt feature was requested, but the OCCT 7.9.3 SDK was not found. \
             The feature flag does not install the SDK. Set OCCT_ROOT to a directory \
             containing include/opencascade and lib, or set OCCT_INCLUDE_DIR and \
             OCCT_LIB_DIR. Build without occt for a drawing-only application."
        );
    };
    let mut build = cc::Build::new();
    build.cpp(true).std("c++17");
    // Treat SDK headers as external while keeping warnings for our bridge.
    // OCCT 7.9.3 uses sprintf in a header, which macOS marks as deprecated.
    if build.get_compiler().is_like_msvc() {
        let mut external_include = std::ffi::OsString::from("/external:I");
        external_include.push(&include);
        build.flag(external_include).flag("/external:W0");
    } else {
        build.flag("-isystem").flag(include.as_os_str());
    }
    build.file("native/bridge.cpp").compile("cam_occt_bridge");
    if let Some(lib) = env::var_os("OCCT_LIB_DIR")
        .map(PathBuf::from)
        .or_else(|| root.map(|p| p.join("lib")))
    {
        println!("cargo:rustc-link-search=native={}", lib.display());
    }
    for lib in [
        "TKDESTEP",
        "TKDEIGES",
        "TKDE",
        "TKXSBase",
        "TKMesh",
        "TKShHealing",
        "TKTopAlgo",
        "TKBRep",
        "TKGeomAlgo",
        "TKGeomBase",
        "TKG3d",
        "TKG2d",
        "TKMath",
        "TKernel",
    ] {
        println!("cargo:rustc-link-lib={lib}");
    }
    println!("cargo:rustc-cfg=occt_available");
}

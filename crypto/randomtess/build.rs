use std::{env, path::PathBuf};

fn main() {
	let source = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap()).join("randomx");
	println!("cargo:rerun-if-changed={}", source.display());

	// The library is always built optimized: a debug build of the VM is orders of magnitude
	// slower and would make block verification impractical in debug node builds.
	let dst = cmake::Config::new(&source)
		.profile("Release")
		.define("ARCH", "default")
		.define("BUILD_SHARED_LIBS", "OFF")
		.build_target("randomx")
		.build();

	let build_dir = dst.join("build");
	println!("cargo:rustc-link-search=native={}", build_dir.display());
	println!("cargo:rustc-link-search=native={}", build_dir.join("Release").display());
	println!("cargo:rustc-link-lib=static=randomx");

	let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap();
	let target_env = env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
	match (target_os.as_str(), target_env.as_str()) {
		("macos" | "ios" | "freebsd" | "openbsd", _) => println!("cargo:rustc-link-lib=dylib=c++"),
		("windows", "msvc") => {},
		_ => println!("cargo:rustc-link-lib=dylib=stdc++"),
	}
	if target_os == "linux" {
		println!("cargo:rustc-link-lib=dylib=pthread");
	}
}

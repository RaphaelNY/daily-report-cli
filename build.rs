fn main() {
    let target = std::env::var("TARGET").expect("TARGET must be set by cargo");
    println!("cargo:rustc-env=DAILY_GIT_BUILD_TARGET={target}");
}

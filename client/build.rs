extern crate cc;

fn main() {
    cc::Build::new()
        .file("jpeg_interface.c")
        .compile("jpeg_interface");
    println!("cargo:rustc-link-lib=dylib=jpeg");
}

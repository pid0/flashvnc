extern crate flashvnc;

fn main() {
    let args : Vec<_> = std::env::args().collect();
    flashvnc::run(args);
}

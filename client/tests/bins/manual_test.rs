extern crate libc;
extern crate tempdir;

use tempdir::TempDir;

#[allow(dead_code)]
mod common;

use common::end_to_end::{VncServer,Client,Build};

fn main() {
    //TODO refactor this
    let args : Vec<_> = std::env::args().skip(1).collect();
    let temp_dir = TempDir::new("flashvnc").unwrap();
    let server = VncServer::start_with_xstartup(temp_dir.path(), r#"
        displayNumber="${DISPLAY:1}"
        mouseServerPort=$((5100 + $displayNumber))
        pkill mouseserver
        mouseserver "$mouseServerPort" &
        exec xterm -e bash --norc
    "#).unwrap();
    let args : Vec<_> = args.iter().map(|s| &s[..]).collect();
    let mut client = Client::start_with_args("localhost", server.port(),
        &args[..], Build::Release, false).unwrap();

    client.wait_for_termination();
}

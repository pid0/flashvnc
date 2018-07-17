// This file is part of flashvnc, a VNC client.
// Copyright 2018 Patrick Plagwitz
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program.  If not, see <http://www.gnu.org/licenses/>.

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

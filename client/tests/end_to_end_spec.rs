extern crate libc;
extern crate tempdir;
extern crate cairo;

#[allow(dead_code)]
mod benchmark;
use benchmark::common::end_to_end::*;
use std::time::Duration;
use tempdir::TempDir;

use std::process::Command;

use std::fs::{File,remove_dir_all,DirBuilder};
use std::path::Path;

fn write_test_image_to(path : &Path) {
    let surface = cairo::ImageSurface::create(cairo::Format::Rgb24, 
                                              TEST_FB_WIDTH as i32, 
                                              TEST_FB_HEIGHT as i32).unwrap();
    let (width, height) = (TEST_FB_WIDTH as f64, TEST_FB_HEIGHT as f64);
    let cr = cairo::Context::new(&surface);
    cr.set_source_rgb(1.0, 0.0, 0.0);
    cr.rectangle(0.0, 0.0, width / 2.0, height);
    cr.fill();

    cr.set_source_rgb(0.2, 0.7, 1.0);
    cr.rectangle(width / 2.0, 0.0, width / 2.0, height);
    cr.fill();

    cr.set_source_rgb(1.0, 1.0, 1.0);
    cr.rectangle(0.0, height - 10.0, width, 10.0);
    cr.fill();
    surface.write_to_png(&mut File::create(path).unwrap()).unwrap();
}

//next: start not simultaneously OR in Xephyr
//      until then: run with target/debug/end_to_end_spec-2820d5f4ff10fef9 --test-threads=1
//then: benchmark animation looks (and is) much slower than test program
//      excessive queuing in client or in server?
//      fps display is also kind of borked: with 10ms delay, it shows 20 fps but this isn't true
#[test]
fn should_communicate_bidirectionally_with_a_vnc_server() {
    let temp_dir = TempDir::new("flashvnc").unwrap();
    let mut server = Server::start(temp_dir.path()).unwrap();
    let client = Client::start("localhost", server.port()).unwrap();

    client.press_mouse(1, 54, 50);
    server.should_have_received_event("press", &["button=1", "x=54", "y=50",
                                      "x_root=54", "y_root=50"], None);

    server.change_screen_size(1152, 864);
    std::thread::sleep(Duration::from_millis(600));
    client.should_have_screen_with_size(1152, 864);
}

#[test]
fn should_show_the_rfb_if_the_client_is_a_still_image() {
    //TODO insecure way of creating a tempdir, but Rust is too shitty to have mktemp
    let temp_dir = ::std::env::temp_dir().join("flashvnc-still-image");
    remove_dir_all(&temp_dir).unwrap_or(());
    DirBuilder::new().create(&temp_dir).unwrap();

    let reference = temp_dir.join("reference.png");
    let actual = temp_dir.join("actual.png");
    let diff = temp_dir.join("diff.png");
    write_test_image_to(&reference);

    let mut server = Server::start(&temp_dir).unwrap();
    server.show_still_image(&reference);
    let client = Client::start("localhost", server.port()).unwrap();

    std::thread::sleep(Duration::from_millis(600));
    client.take_screenshot(&actual);

    assert!(Command::new("compare")
        .args(&["-metric", "PSNR"])
        .args(&[reference, actual, diff])
        .status().unwrap().success());
}

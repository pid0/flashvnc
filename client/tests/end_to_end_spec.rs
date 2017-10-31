extern crate libc;
extern crate tempdir;
extern crate cairo;

#[allow(dead_code)]
#[macro_use]
mod bins;
use bins::common::end_to_end::*;
use std::time::Duration;
use tempdir::TempDir;

use std::process::Command;

use std::fs::{File,remove_dir_all,DirBuilder};
use std::path::Path;

fn write_three_color_test_image(cr : &mut cairo::Context, 
                                width : f64, height : f64)
{
    cr.set_source_rgb(1.0, 0.0, 0.0);
    cr.rectangle(0.0, 0.0, width / 2.0, height);
    cr.fill();

    cr.set_source_rgb(0.2, 0.7, 1.0);
    cr.rectangle(width / 2.0, 0.0, width / 2.0, height);
    cr.fill();

    cr.set_source_rgb(1.0, 1.0, 1.0);
    cr.rectangle(0.0, height - 10.0, width, 10.0);
    cr.fill();
}
fn write_all_colors_image(cr : &mut cairo::Context, width : f64, height : f64) {
    let width = width as u32;
    let height = height as u32;
    let mut n = 0u32;
    for y in 0..height {
        for x in 0..width {
            n += 1;
            let r = (n & 0x00ff0000) >> 16;
            let g = (n & 0x0000ff00) >> 8;
            let b = n & 0x000000ff;
            cr.set_source_rgb(r as f64 / 15.0, g as f64 / 255.0, 
                              b as f64 / 255.0);
            cr.rectangle(x as f64, y as f64, 1.0, 1.0);
            cr.fill();
        }
    }
}

fn assert_eq_images(reference : &Path, actual : &Path, diff : &Path,
                    exact : bool)
{
    let output = Command::new("compare")
        .args(&["-metric", "mse"])
        .args(&[reference, actual, diff])
        .output().unwrap();
    if output.status.success() {
        return;
    }
    assert!(!exact);
    let output = String::from_utf8(output.stderr).unwrap();
    let words : Vec<&str> = output.split_whitespace().collect();
    let mean_squared_error : f64 = words[0].parse().unwrap();

    assert!(mean_squared_error < 6.0, "{}", mean_squared_error);
}

fn should_correctly_show_image<F>(server : &mut Server, client : &Client,
                                  dir : &Path, name : &str, generate_image : F,
                                  exact : bool)
    where F : Fn(&mut cairo::Context, f64, f64) -> ()
{
    let reference = dir.join(&format!("{}-reference.png", name));
    let actual = dir.join(&format!("{}-actual.png", name));
    let diff = dir.join(&format!("{}-diff.png", name));

    let surface = cairo::ImageSurface::create(cairo::Format::Rgb24, 
                                              TEST_FB_WIDTH as i32, 
                                              TEST_FB_HEIGHT as i32).unwrap();
    generate_image(&mut cairo::Context::new(&surface), 
                   TEST_FB_WIDTH as f64, TEST_FB_HEIGHT as f64);
    surface.write_to_png(&mut File::create(&reference).unwrap()).unwrap();

    server.show_still_image(&reference);
    std::thread::sleep(Duration::from_millis(1200));
    client.take_screenshot(&actual);

    assert_eq_images(&reference, &actual, &diff, exact);
}

fn setup() -> (TempDir, Server, Client) {
    let temp_dir = TempDir::new("flashvnc").unwrap();
    let server = Server::start(temp_dir.path()).unwrap();
    let client = Client::start("localhost", server.port()).unwrap();
    (temp_dir, server, client)
}

//next: start not simultaneously OR in Xephyr
//      until then: run with target/debug/end_to_end_spec-2820d5f4ff10fef9 --test-threads=1
//then: benchmark animation looks (and is) much slower than test program
//      excessive queuing in client or in server?
//      fps display is also kind of borked: with 10ms delay, it shows 20 fps but this isn't true
#[test]
fn should_communicate_bidirectionally_with_a_vnc_server() {
    let (_, mut server, client) = setup();

    client.press_mouse_at(1, 54, 50);
    server.should_have_received_event("press", &["button=1", "x=54", "y=50",
                                      "x_root=54", "y_root=50"], None);

    server.change_screen_size(1152, 864);
    std::thread::sleep(Duration::from_millis(800));
    client.should_have_screen_with_size(1152, 864);
}

#[test]
fn should_show_the_rfb_if_the_client_is_a_still_image() {
    //TODO insecure way of creating a tempdir, but Rust is too shitty to have mktemp
    let temp_dir = ::std::env::temp_dir().join("flashvnc-still-image");
    remove_dir_all(&temp_dir).unwrap_or(());
    DirBuilder::new().create(&temp_dir).unwrap();

    let mut server = Server::start(&temp_dir).unwrap();
    let client = Client::start("localhost", server.port()).unwrap();

    client.turn_on_lossless_compression();
    should_correctly_show_image(&mut server, &client, &temp_dir, "all-colors",
                                write_all_colors_image, true);
    should_correctly_show_image(&mut server, &client, &temp_dir, "three-color",
                                write_three_color_test_image, true);

    client.turn_on_lossy_compression();
    should_correctly_show_image(&mut server, &client, &temp_dir, "all-colors",
                                write_all_colors_image, false);
}

#[test]
fn should_intercept_keyboard_events_and_send_them_to_the_server() {
    let temp_dir = TempDir::new("flashvnc").unwrap();
    let server = VncServer::start_with_xstartup(temp_dir.path(),
        &format!(
        r#"
        cd {}
        exec xterm -e bash --norc
        "#, temp_dir.path().to_str().unwrap())).unwrap();
    let client = Client::start("localhost", server.port()).unwrap();

    File::create(temp_dir.path().join("A")).unwrap();

    std::thread::sleep(Duration::from_millis(200));
    client.focus();

    client.press_key(KEY_SHIFT_L);
    client.press_and_release_key(KEY_LOWER_B);
    client.release_key(KEY_SHIFT_L);

    client.press_key(KEY_CTRL_L);
    client.press_and_release_key(KEY_LOWER_U);
    client.release_key(KEY_CTRL_L);

    client.press_and_release_key(KEY_LOWER_C);
    client.press_and_release_key(KEY_LOWER_P);
    client.press_and_release_key(KEY_SPACE);
    client.press_and_release_key(KEY_UPPER_A);
    client.press_and_release_key(KEY_SPACE);

    client.press_key(KEY_CTRL_L);
    client.press_and_release_key(KEY_LOWER_Y);
    client.release_key(KEY_CTRL_L);

    client.press_and_release_key(KEY_RETURN);

    assert_eq_with_timeout!(1000, temp_dir.path().join("B").is_file(), true);
}

#[test]
fn should_intercept_mouse_button_and_move_events_and_send_them() {
    //TODO refactor set-up
    let temp_dir = TempDir::new("flashvnc").unwrap();
    let server = VncServer::start_with_xstartup(temp_dir.path(),
        &format!(
        r#"
        cd {}
        PS1='touch ABC\nexit' xterm -e bash --norc
        sleep 2
        "#, temp_dir.path().to_str().unwrap())).unwrap();
    let client = Client::start("localhost", server.port()).unwrap();

    std::thread::sleep(Duration::from_millis(1300));
    client.press_mouse_at(1, 1, 1);
    client.release_mouse_at(1, 1, 30);
    client.click_mouse_at(2, 50, 50);

    assert_eq_with_timeout!(1000, temp_dir.path().join("ABC").is_file(), true);
}

#[test]
fn should_make_the_server_change_fb_size_when_the_window_is_resized() {
    let temp_dir = TempDir::new("flashvnc").unwrap();
    let mut server = Server::start(temp_dir.path()).unwrap();
    let client = Client::start("localhost", server.port()).unwrap();

    std::thread::sleep(Duration::from_millis(300));
    client.resize_window_content(900, 456);

    server.should_have_screen_size(900, 456);
}

#[test]
fn should_be_able_to_send_relative_mouse_motion_events() {
    let (_, mut server, client) = setup();

    client.position_mouse(100, 50);
    assert_eq_with_timeout!(1000, server.mouse_position(), (100, 50));

    client.turn_on_relative_mouse_mode();
    server.set_mouse_position(10, 10);
    assert_eq_with_timeout!(1000, server.mouse_position(), (10, 10));
    std::thread::sleep(Duration::from_millis(350));
    client.move_mouse(-6, 4);
    assert_eq_with_timeout!(1000, server.mouse_position(), (4, 14));

    client.press_mouse(1);
    server.should_have_received_event("press", &["button=1", "x=4", "y=14"], 
                                      None);
}

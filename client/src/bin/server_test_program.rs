#![feature(drop_types_in_const)]
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


use std::process::Command;

extern crate gtk;
extern crate gdk;
extern crate gdk_pixbuf;
extern crate glib;
extern crate libc;
extern crate cairo;

use cairo::Gradient;

use gtk::{WidgetExt,WindowExt,ContainerExt};
use gdk::WindowExt as GdkWindowExt;
use gdk::{ContextExt,DisplayExt,SeatExt,DeviceExt};

use gdk_pixbuf::Pixbuf;

use std::io;
use std::cell::RefCell;
use std::sync::{Arc,Mutex};
use std::str::FromStr;

static mut DRAWING_AREA : Option<gtk::DrawingArea> = None;
fn get_drawing_area() -> &'static gtk::DrawingArea {
    unsafe { DRAWING_AREA.as_mut().unwrap() }
}

struct CursorPos {
    x : u32,
    y : u32
}

fn set_expand<T : gtk::WidgetExt>(widget : &T) {
    widget.set_hexpand(true);
    widget.set_vexpand(true);
}

fn handle_input(_widget : &gtk::DrawingArea, e : &gdk::EventButton) 
    -> gtk::Inhibit 
{
    let (x, y) = e.get_position();
    let (x_root, y_root) = e.get_window().unwrap().get_root_coords(
        x as i32, y as i32);
    let event_type = match e.get_event_type() {
        gdk::EventType::ButtonRelease => "release",
        gdk::EventType::DoubleButtonPress => "2press",
        gdk::EventType::TripleButtonPress => "3press",
        gdk::EventType::ButtonPress => "press",
        _ => "?"
    };
    //e.get_state?
    println!("{} button={} x={} y={} x_root={} y_root={}", 
                event_type,
                e.get_button(),
                x as i32, y as i32, x_root, y_root);
    gtk::Inhibit(true)
}

fn serve(benchmark_mode : Arc<Mutex<bool>>, 
         image_file_path : Arc<Mutex<String>>,
         cursor_pos : Arc<Mutex<CursorPos>>)
{
        //std::thread::sleep(std::time::Duration::from_secs(1));
    let mut line = String::new();
    let stdin = io::stdin();
    loop {
        line.clear();
        stdin.read_line(&mut line).unwrap();
        let words : Vec<&str> = line.split_whitespace().collect();
        
        match words[0] {
            //TODO change window size, too?
            "change-screen-size" => {
                Command::new("xrandr")
                    .args(&["-s", words[1]])
                    .status()
                    .expect("xrandr");
            }
            "show-benchmark" => {
                *benchmark_mode.lock().unwrap() = true;
                glib::idle_add(move || {
                    get_drawing_area().queue_draw();
                    glib::Continue(false)
                });
            }
            "show-image" => {
                //TODO use rest of words
                *image_file_path.lock().unwrap() = String::from(words[1]);
                glib::idle_add(move || {
                    get_drawing_area().queue_draw();
                    glib::Continue(false)
                });
            }
            "query-screen-size" => {
                let width = Command::new("./screen-size.sh")
                    .arg("Width")
                    .output().unwrap().stdout;
                let width = String::from_utf8(width).unwrap();
                let height = Command::new("./screen-size.sh")
                    .arg("Height")
                    .output().unwrap().stdout;
                let height = String::from_utf8(height).unwrap();
                println!("{} {}", width.trim(), height.trim());
            }
            "query-mouse-position" => {
                let pos = cursor_pos.lock().unwrap();
                println!("{} {}", pos.x, pos.y);
            }
            "set-mouse-position" => {
                let (x, y) = (i32::from_str(words[1]).unwrap(),
                              i32::from_str(words[2]).unwrap());
                glib::idle_add(move || {
                    let display = gdk::Display::get_default().unwrap();
                    let seat = display.get_default_seat().unwrap();
                    let mouse = seat.get_pointer().unwrap();
                    mouse.warp(&display.get_default_screen(), x, y);
                    glib::Continue(false)
                });
            }
            _ => { 
                unimplemented!()
            }
        };
//        glib::idle_add(|| {
//            eprintln!("lala");
//            glib::Continue(false)
//        });
    }
}

fn main() {
    if gtk::init().is_err() {
        eprintln!("Failed to initialize GTK");
        std::process::exit(1);
    }

    let window = gtk::Window::new(gtk::WindowType::Toplevel);

    let area = gtk::DrawingArea::new();
    set_expand(&area);
    window.add(&area);

    struct Animation {
        forwards : bool,
        center_pos : f64
    }
    struct DrawData {
        animation : RefCell<Animation>,
        image : RefCell<Option<Pixbuf>>
    }
    let data = DrawData {
        animation: RefCell::new(Animation {
            forwards: true,
            center_pos: 0.0
        }),
        image: RefCell::new(None)
    };

    let benchmark_mode = Arc::new(Mutex::new(false));
    let image_file_path = Arc::new(Mutex::new(String::new()));
    let cursor_pos = Arc::new(Mutex::new(CursorPos { x: 0, y: 0 }));
    let benchmark_mode_clone = benchmark_mode.clone();
    let image_file_path_clone = image_file_path.clone();
    let cursor_pos_clone = cursor_pos.clone();
    let last_image_path = RefCell::new(String::new());

    area.connect_draw(move |ref area, ref cr| {
        let width = area.get_allocated_width() as f64;
        let height = area.get_allocated_height() as f64;
//        cr.set_source_rgb(1.0, 0.0, 0.0);
//        cr.rectangle(0.0, 0.0, width / 2.0, height);
//        cr.fill();
//        cr.set_source_rgb(0.0, 1.0, 0.0);
//        cr.rectangle(width / 2.0, 0.0, width / 2.0, height);
//        cr.fill();

        if image_file_path.lock().unwrap().len() > 0 {
            let mut last_image_path = last_image_path.borrow_mut();
            let mut image = data.image.borrow_mut();
            if image.is_none() 
                || *image_file_path.lock().unwrap() != *last_image_path {
                *image = Some(Pixbuf::new_from_file(
                         &image_file_path.lock().unwrap()).unwrap());
                *last_image_path = image_file_path.lock().unwrap().clone();
            }
            cr.set_source_pixbuf(image.as_ref().unwrap(), 0.0, 0.0);
            cr.rectangle(0.0, 0.0, width, height);
            cr.fill();
            return gtk::Inhibit(true);
        }

        let mut animation = data.animation.borrow_mut();

        let gradient = cairo::LinearGradient::new(0.0, 0.0, width, 0.0);
        gradient.add_color_stop_rgb(0.0, 1.0, 1.0, 1.0);
        gradient.add_color_stop_rgb(animation.center_pos, 0.0, 0.0, 1.0);
        gradient.add_color_stop_rgb(1.0, 1.0, 1.0, 1.0);
        cr.set_source(&gradient);
        cr.rectangle(0.0, 0.0, width, height);
        cr.fill();

        if *benchmark_mode.lock().unwrap() {
            //::std::thread::sleep(::std::time::Duration::from_millis(10));
//            eprintln!("animation pos: {}", animation.center_pos);
            if animation.forwards {
                animation.center_pos += 0.009;
                if animation.center_pos >= 1.0 {
                    animation.forwards = false;
                }
            } else {
                animation.center_pos -= 0.009;
                if animation.center_pos <= 0.0 {
                    animation.forwards = true;
                }
            }
            area.queue_draw();
        }
        gtk::Inhibit(true)
    });
//    glib::timeout_add(50, move || {
//        area_clone.queue_draw();
//        glib::Continue(true)
//    });

    area.connect_button_press_event(|ref widget, ref e| {
        handle_input(widget, e)
    });
    area.connect_button_release_event(|ref widget, ref e| {
        handle_input(widget, e)
    });
    area.connect_motion_notify_event(move |ref _widget, ref e| {
        let (x, y) = e.get_position();
        let mut pos = cursor_pos.lock().unwrap();
        pos.x = x as u32;
        pos.y = y as u32;
        gtk::Inhibit(false)
    });
    let mut event_mask = gdk::EventMask::from_bits_truncate(
        area.get_events() as u32);
    event_mask.insert(gdk::BUTTON_PRESS_MASK);
    event_mask.insert(gdk::BUTTON_RELEASE_MASK);
    event_mask.insert(gdk::POINTER_MOTION_MASK);
    area.set_events(event_mask.bits() as i32);

    window.set_size_request(1366, 768);

    println!("{}", unsafe { libc::getpid() });

    unsafe {
        DRAWING_AREA = Some(area.clone());
    }
    std::thread::spawn(move || { 
        serve(benchmark_mode_clone, image_file_path_clone, cursor_pos_clone)
    });
    std::thread::spawn(|| {
        loop {
            std::thread::sleep(std::time::Duration::from_millis(500));
            println!("");
        }
    });

    window.set_title("rustprog");
    //window.fullscreen();

    window.connect_delete_event(|_, _| {
        gtk::main_quit();
        gtk::Inhibit(false)
    });
    window.show_all();
    gtk::main();
}

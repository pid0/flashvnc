use std::process::Command;

extern crate gtk;
extern crate gdk;
//extern crate glib;
extern crate libc;

use gtk::WidgetExt;
use gtk::WindowExt;
use gtk::ContainerExt;

use std::io;

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

fn serve() {
        //std::thread::sleep(std::time::Duration::from_secs(1));
    let mut line = String::new();
    let stdin = io::stdin();
    loop {
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

    area.connect_draw(move |ref area, ref cr| {
        let width = area.get_allocated_width() as f64;
        let height = area.get_allocated_height() as f64;
        cr.set_source_rgb(1.0, 0.0, 0.0);
        cr.rectangle(0.0, 0.0, width / 2.0, height);
        cr.fill();
        cr.set_source_rgb(0.0, 1.0, 0.0);
        cr.rectangle(width / 2.0, 0.0, width / 2.0, height);
        cr.fill();
        gtk::Inhibit(true)
    });

    area.connect_button_press_event(|ref widget, ref e| {
        handle_input(widget, e)
    });
    area.connect_button_release_event(|ref widget, ref e| {
        handle_input(widget, e)
    });
    //TODO key press/release
    let mut event_mask = gdk::EventMask::from_bits_truncate(
        area.get_events() as u32);
    event_mask.insert(gdk::BUTTON_PRESS_MASK);
    event_mask.insert(gdk::BUTTON_RELEASE_MASK);
    area.set_events(event_mask.bits() as i32);

    window.set_size_request(800, 600);

    println!("{}", unsafe { libc::getpid() });

    std::thread::spawn(serve);
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

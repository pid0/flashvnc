use ::{GuiEvent,ProtocolEvent,socket_thread_main,View,ConnectionConfig,
       MainError};

use gtk;
use gdk;
use std;
use glib;
use gtk::WidgetExt;
use gtk::WindowExt;
use gtk::ContainerExt;
use gtk::DialogExt;

use gdk::prelude::ContextExt;

use gdk_pixbuf::Pixbuf;

use std::sync::mpsc;
use std::cell::RefCell;

struct GtkContext {
    connection_in : mpsc::Receiver<ProtocolEvent>,
    window : gtk::Window,
    drawing_area : gtk::DrawingArea,
    pixbuf : RefCell<Pixbuf>
}
static mut GTK_CONTEXT : Option<GtkContext> = None;
fn gtk_context() -> &'static GtkContext {
    unsafe { GTK_CONTEXT.as_ref().unwrap() }
}
static mut CONNECTION_OUT : Option<mpsc::Sender<GuiEvent>> = None;
fn connection_out() -> &'static mpsc::Sender<GuiEvent> {
    unsafe { CONNECTION_OUT.as_ref().unwrap() }
}

fn handle_protocol_event(event : ProtocolEvent) {
    let drawing_area = &gtk_context().drawing_area;
    let window = &gtk_context().window;
    match event {
        ProtocolEvent::ChangeFbSize(size) => {
            drawing_area.set_size_request(size.width as i32,
                                          size.height as i32);
            window.resize(1, 1);
        },
        ProtocolEvent::UpdateFramebuffer(new_fb, size) => {
//            eprintln!("updating GUI framebuffer {:?}", ::std::time::Instant::now());
            let fb_copy = new_fb.read().unwrap().clone();
            let pixbuf = Pixbuf::new_from_vec(fb_copy, 0, false, 8, 
                                              size.width as i32,
                                              size.height as i32,
                                              3 * (size.width as i32)); //TODO use constant for 3
            *gtk_context().pixbuf.borrow_mut() = pixbuf;
            gtk_context().drawing_area.queue_draw();
//            eprintln!("updated GUI framebuffer {:?}", ::std::time::Instant::now());
        }
    }
}
struct GtkView {
    events_in : mpsc::Receiver<GuiEvent>,
    events_out : mpsc::Sender<ProtocolEvent>
}
impl View for GtkView {
    fn handle_event(&self, event : ProtocolEvent) {
        self.events_out.send(event).unwrap();
        glib::idle_add(|| {
            while let Ok(event) = gtk_context().connection_in.try_recv() {
                handle_protocol_event(event)
            }
            glib::Continue(false)
        });
    }
    fn get_events(&self) -> &mpsc::Receiver<GuiEvent> {
        &self.events_in
    }
}

fn set_expand<T : gtk::WidgetExt>(widget : &T) {
    widget.set_hexpand(true);
    widget.set_vexpand(true);
}

fn show_fatal_error(error_string : String) {
    //TODO set parent window
    //TODO also print to stderr
    glib::idle_add(move || {
        let message = format!("Connection closed due to an error:\n{}",
                              error_string);
        eprintln!("{}", message);
        let message_box = gtk::MessageDialog::new::<gtk::Window>(
            None,
            gtk::DIALOG_MODAL,
            gtk::MessageType::Error,
            gtk::ButtonsType::Close,
            &message);
        message_box.run();
        message_box.destroy();
        gtk::main_quit();
        //TODO exit with 1
        glib::Continue(false)
    });
}

fn handle_input(_widget : &gtk::DrawingArea, e : &gdk::EventButton) 
    -> gtk::Inhibit 
{
    let (x, y) = e.get_position();
//    let (x_root, y_root) = e.get_window().unwrap().get_root_coords(
//        x as i32, y as i32);
//    let event_type = match e.get_event_type() {
//        gdk::EventType::ButtonRelease => "release",
//        gdk::EventType::ButtonPress => "press",
////        gdk::EventType::DoubleButtonPress => "2press",
////        gdk::EventType::TripleButtonPress => "3press",
//        _ => "?"
//    };
    //TODO distinguish between press and release
    //TODO use get_state: see
    //https://people.gnome.org/~gcampagna/docs/Gdk-3.0/Gdk.ModifierType.html
    connection_out().send(
        GuiEvent::Pointer {
            state: 1 << (e.get_button() - 1),
            mask: 0xff,
            x: x as i32,
            y: y as i32
        }).unwrap();
//    //e.get_state?
    gtk::Inhibit(true)
}

pub fn run(config : ConnectionConfig) {
    if gtk::init().is_err() {
        eprintln!("Failed to initialize GTK");
        std::process::exit(1);
    }

    let window = gtk::Window::new(gtk::WindowType::Toplevel);

    let area = gtk::DrawingArea::new();
    set_expand(&area);
    window.add(&area);

//    let colorspace_rgb = 0;
//    let pixbuf = unsafe { gdk_pixbuf::Pixbuf::new(
//            colorspace_rgb, false, 8, 50, 50) }.unwrap();
//    let pixels = unsafe { pixbuf.get_pixels() };
//    let row_size = pixbuf.get_rowstride() as usize;
//    for y in 0..50 {
//        for x in 0..25 {
//            pixels[row_size * y + 3 * x] = 255;
//            pixels[row_size * y + 3 * x + 1] = 0;
//            pixels[row_size * y + 3 * x + 2] = 0;
//        }
//        for x in 25..50 {
//            pixels[row_size * y + 3 * x] = 0;
//            pixels[row_size * y + 3 * x + 1] = 255;
//            pixels[row_size * y + 3 * x + 2] = 0;
//        }
//    }
//
//    let pixbuf_clone = pixbuf.clone();
    area.connect_draw(move |ref area, ref cr| {
        let width = area.get_allocated_width() as f64;
        let height = area.get_allocated_height() as f64;
//        let surface = gdk::cairo_sur...
//        cr.set_source_surface(&surface, 0, 0);
//        let pixbuf = gdk_pixbuf::Pixbuf::new_from_file("/tmp/flashvnc-still-image/reference.png").unwrap();
//        cr.set_source_pixbuf(&pixbuf, 0.0, 0.0);

//        cr.set_source_pixbuf(&pixbuf_clone, 0.0, 0.0);
        cr.set_source_pixbuf(&gtk_context().pixbuf.borrow(), 0.0, 0.0);
        cr.rectangle(0.0, 0.0, width, height);
        cr.fill();
//        eprintln!("drawing GUI framebuffer {:?}", ::std::time::Instant::now());
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

    let (gui_events_tx, gui_events_rx) = mpsc::channel();
    let (protocol_events_tx, protocol_events_rx) = mpsc::channel();
    unsafe {
        CONNECTION_OUT = Some(gui_events_tx);
    }
    let one_pixel_fb = vec![0xff, 0xff, 0xff];
    unsafe {
        GTK_CONTEXT = Some(GtkContext {
            connection_in: protocol_events_rx,
            window: window.clone(),
            drawing_area: area.clone(),
            pixbuf: RefCell::new(Pixbuf::new_from_vec(
                    one_pixel_fb, 0, false, 8, 1, 1, 3)) //TODO use constant
        });
    }
    let view = GtkView {
        events_in: gui_events_rx,
        events_out: protocol_events_tx
    };
    std::thread::spawn(move || {
        let main_result = socket_thread_main(config, view);

        if let Err(MainError(error_message)) = main_result {
            show_fatal_error(error_message);
        }
        //TODO call main_quit
    });

    area.set_size_request(800, 600);
    area.set_size_request(534, 600);

    window.set_title("flashvnc");
    //window.fullscreen();

    window.connect_delete_event(|_, _| {
        gtk::main_quit();
        gtk::Inhibit(false)
    });
    window.show_all();
    gtk::main();
}

#![feature(conservative_impl_trait)]
#![feature(const_fn)]
#![feature(drop_types_in_const)]

extern crate gtk;
extern crate gdk;
extern crate glib;
extern crate gdk_pixbuf;
#[macro_use]
extern crate derivative;

//use std::io;
//use std::io::Read;
//use std::io::{Read,Write};
use std::net::TcpStream;
use std::sync::mpsc;
//use std::io as abcdef;

use gtk::WidgetExt;
use gtk::WindowExt;
use gtk::ContainerExt;
use gtk::DialogExt;

use gdk::prelude::ContextExt;

pub mod protocol;

use protocol::rfb;
use protocol::parsing::io_input::SharedBuf;
use protocol::parsing::Packet;
use std::cell::RefCell;

pub enum GuiEvent {
    Pointer {
        state : u8,
        mask : u8,
        //TODO f64 for relative mouse movement?
        x : i32,
        y : i32
    },
    Foo
}

struct GtkContext {
    connection_in : mpsc::Receiver<ProtocolEvent>,
    window : gtk::Window,
    drawing_area : gtk::DrawingArea,
}
static mut GTK_CONTEXT : Option<GtkContext> = None;
fn gtk_context() -> &'static GtkContext {
    unsafe { GTK_CONTEXT.as_ref().unwrap() }
}

//TODO rename to CONNECTION_OUT
static mut CONNECTION_OUT : Option<mpsc::Sender<GuiEvent>> = None;
fn connection_out() -> &'static mpsc::Sender<GuiEvent> {
    unsafe { CONNECTION_OUT.as_ref().unwrap() }
}

pub fn parse_packet<T>(buffer : &SharedBuf, input : &TcpStream) 
    -> Result<T, String>
    where T : Packet
{
    let ret = T::parse(buffer, input);
    match ret {
        Ok(x) => Ok(x),
        Err((err, position)) => if err.is_eof() {
            Err(String::from("Connection to server broken"))
        } else {
            Err(format!(
                    "Error at position {} when parsing packet {}: {:?}",
                    position, T::name(), err))
        }
    }
}

pub fn write_packet<T>(packet : T, output : &mut TcpStream)
    -> Result<(), String>
    where T : Packet
{
    match packet.write(output) {
        Ok(()) => Ok(()),
        Err(err) => if err.is_io_error() {
            Err(String::from(
                "Can't write packet, connection to server broken"))
        } else {
            Err(format!("Can't write packet {}: {:?}",
                        T::name(), err))
        }
    }
}

pub fn socket_thread_main<V : View>(
    host_and_port : &str,
    view : V) -> Result<(), String>
{
    //TODO handle differently (re-connect)?
    let socket = match TcpStream::connect(host_and_port) {
        Ok(s) => s,
        Err(err) => {
            //TODO implement From and then Display for MainError
            return Err(format!("Could not connect to {}: {}", 
                               host_and_port, err));
        }
    };

    //TODO benchmark BufferedReader
    handle_connection(socket, view)
}

//TODO move to different file
pub struct FbSize {
    width: u16, 
    height: u16
}
impl FbSize {
    pub fn new(width : u16, height : u16) -> Self {
        Self {
            width: width,
            height: height
        }
    }
}
pub enum ProtocolEvent {
    ChangeFbSize(FbSize)
}
pub trait View {
    fn change_fb_size_to(&self, size : FbSize) {
        self.handle_event(ProtocolEvent::ChangeFbSize(size));
    }

    fn handle_event(&self, event : ProtocolEvent);
    fn get_events(&self) -> &mpsc::Receiver<GuiEvent>;
}

//TODO move to different file (application/gtk.rs)
fn handle_protocol_event(event : ProtocolEvent) {
    let drawing_area = &gtk_context().drawing_area;
    let window = &gtk_context().window;
    match event {
        ProtocolEvent::ChangeFbSize(size) => {
            drawing_area.set_size_request(size.width as i32,
                                          size.height as i32);
            window.resize(1, 1);
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

pub fn handle_connection<V : View>(
    mut socket : TcpStream, 
    view : V) -> Result<(), String>
{
    let server_address = socket.peer_addr().unwrap();
    let buffer = RefCell::new(Vec::new());

    let protocol_version = parse_packet::<rfb::ProtocolVersion>(
        &buffer, &socket)?;
    write_packet(protocol_version, &mut socket)?;

    let security_types = 
        match parse_packet::<rfb::SecurityTypes>(&buffer, &socket)? {
            rfb::SecurityTypes::ErrorReason(error_message) => 
                return Err(format!(
                        "Server error after version negotiation:\n{}", 
                        error_message.string)),
            rfb::SecurityTypes::SecurityTypesArray(x) => x
        };
    if !security_types.types.iter().any(|&x| x == rfb::SEC_TYPE_NONE) {
        return Err(String::from(
                "Server requires authentication. Not implemented yet."))
    }
    
    //eprintln!("received[{}] ‘{:?}’", server_address, security_types.types);
    write_packet(rfb::SecurityResponse {
        sec_type: rfb::SEC_TYPE_NONE
    }, &mut socket)?;

    let security_result = parse_packet::<rfb::SecurityResult>(
        &buffer, &socket)?;
    if let rfb::SecurityResult::Failed(reason) = security_result {
        return Err(format!("Failed security handshake: {}", reason.string));
    }

    write_packet(rfb::ClientInit {
        shared: false
    }, &mut socket)?;

    let server_init = parse_packet::<rfb::ServerInit>(&buffer, &socket)?;
//    eprintln!("received[{}] ‘{:?}’", server_address, server_init);
    //‘ServerInit { 
    //width: 1240, height: 900, 
    //pixel_format: PixelFormat { 
    //bits_per_pixel: 32,
    //depth: 24, big_endian: false, true_color: true, 
    //red_max: 255, green_max: 255, blue_max: 255,
    //red_shift: 16, green_shift: 8, blue_shift: 0 }, 
    //name: "TurboVNC: PatrickDesktop:2 (patrick)"
    //}’
    let fb_width = server_init.width;
    let fb_height = server_init.height;
    //TODO call change_fb_size_to here

//                   +--------+--------------------------+
//                   | Number | Name                     |
//                   +--------+--------------------------+
//                   | 0      | SetPixelFormat           |
//                   | 2      | SetEncodings             |
//                   | 3      | FramebufferUpdateRequest |
//                   | 4      | KeyEvent                 |
//                   | 5      | PointerEvent             |
//                   | 6      | ClientCutText            |
//                   +--------+--------------------------+
    //TODO SetPixelFormat however you want (easy in client, hard only in server)
    //TODO next: now, the client must send something
	// SetEncodings with nothing (raw is still allowed) (necessary for skeleton?)
	// Complete FramebufferUpdateRequest (not necessary)
    // KeyEvent
    
    //TODO ClientToServer/ServerToClient main part of protocol into own function

    //TODO keep button mask as state
    //note: TurboVNC then uses a protocol writer with a mutex
    
    write_packet(rfb::ClientToServer::SetEncodings(rfb::SetEncodings {
        encodings: vec![
            rfb::ENCODING_RAW,
//            rfb::ENCODING_TIGHT,
            rfb::ENCODING_DESKTOP_SIZE]
    }), &mut socket)?;
    
//    write_packet(rfb::ClientToServer::PointerEvent(rfb::PointerEvent {
//        mask: 1, //button 1 (button 3 == 4)
//        x: 5,
//        y: 7
//    }), &mut socket)?;

    //TODO non-blocking select between GUI and server? (use try_recv for mpsc)
    //next: current dep -> prefix_dep, then: one_way_dep/dep
    //TODO first: decode everything here, then: use worker threads and *benchmark this*!
    //TODO document in TODO: use input.read_into to avoid copying, but benchmark this!
    loop {
        match view.get_events().recv().unwrap() {
            //TODO update button mask as state
            GuiEvent::Pointer { state, mask: _, x, y } => {
                write_packet(rfb::ClientToServer::PointerEvent(
                        rfb::PointerEvent {
                            mask: state,
                            x: x as u16, //TODO clamp
                            y: y as u16
                        }), &mut socket)?;
            },
            _ => { }
        }

        //TODO do not block when waiting for the FramebufferUpdate
        loop {
        write_packet(rfb::ClientToServer::FramebufferUpdateRequest(
                rfb::FramebufferUpdateRequest {
                    incremental: false,
                    x: 0,
                    y: 0,
                    width: fb_width,
                    height: fb_height
                }), &mut socket)?;
        let _raw_rect_size = 5;
        match parse_packet::<rfb::ServerToClient>(&buffer, &socket)? {
            rfb::ServerToClient::FramebufferUpdate(update) => {
//                eprintln!("received[{}] ‘{:?}’", server_address, update);
                for _ in 0..update.no_of_rectangles {
                    let rectangle = parse_packet::<rfb::Rectangle>(
                        &buffer, &socket)?;
//                    eprintln!("received[{}] ‘{:?}’", server_address, rectangle);
                    match rectangle.payload {
                        //TODO refactor to e.g. rfb::rect::Raw with a pub type
                        //TODO test should_request_the_full_framebuffer_after_initialization and
                        //check if handled correctly
                        //next:
                        //x. LazyVec -> make compile (minus skip and change_fb_size and errors)
                        //x. implement change_fb_size
                        //x. pass walking skeleton
                        //4. write benchmark, first without any pixel data, just a loop
                        //5. write test above as end-to-end test, take a screenshot. Write the
                        //   benchmark while passing this test.
                        //6. implement skip?, handle errors for manual I/O operations (see TODO above)
                        //7. implement Tight encoding
                        //8. implement JPEG encoding (is Tight?)
                        rfb::RectanglePayload::RawRectangle(_) => {
                            //TODO implement
                            //TODO implement a skip function
                            //TODO handle error
                            let size = ((server_init.pixel_format
                                .bits_per_pixel as usize) / 8)
                                * (rectangle.width as usize)
                                * (rectangle.height as usize);
                            let size = size as usize;
                            let mut bytes = Vec::with_capacity(size);
                            unsafe {
                                bytes.set_len(size);
                            }
                            ::std::io::Read::read_exact(&mut socket, &mut bytes[..]).unwrap();
//                            unimplemented!()
                        },
                        rfb::RectanglePayload::TightRectangle(_tight) => {
//                            ::std::io::Read::read_exact(&mut socket, &mut bytes[..]).unwrap();
//                            eprintln!("received[{}] tight: ‘{:?}’", server_address, byte);
                            unimplemented!()
                        },
                        rfb::RectanglePayload::DesktopSizeRectangle(_) => {
                            eprintln!("received[{}] ‘{:?}’", server_address, rectangle);
                            view.change_fb_size_to(
                                FbSize::new(rectangle.width, rectangle.height));
                        }
                    }
                }
            },
            _ => { }
        }
        }
    }

    //for a click:
//    write_packet(rfb::ClientToServer::PointerEvent(rfb::PointerEvent {
//        mask: 0,
//        x: 5,
//        y: 7
//    }), &mut socket)?;
    

//    let mut buf = [0u8; 1];
//    socket.read_exact(&mut buf[..]).unwrap();
//    eprintln!("received[{}] ‘{:?}’", server_address, buf);

//	write_packet(rfb::ClientToServer::FramebufferUpdateRequest(
//            rfb::FramebufferUpdateRequest {
//                incremental: false,
//                x: 0,
//                y: 0,
//                width: width,
//                height: height
//            }), &mut socket);
//    write_packet(rfb::ClientToServer::KeyEvent(rfb::KeyEvent {
//        down: true,
//        key: ?? //see doc
//    }), &mut socket);

	//TODO later: request pseudo-encodings
    //TODO call exit on view instead
    //TODO document: ClientCutText not supported
//    glib::idle_add(|| {
//        gtk::main_quit();
//        glib::Continue(false)
//    });

//    Ok(())
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

pub fn run(args : Vec<String>) {
    if gtk::init().is_err() {
        eprintln!("Failed to initialize GTK");
        std::process::exit(1);
    }

    let window = gtk::Window::new(gtk::WindowType::Toplevel);

    let area = gtk::DrawingArea::new();
    set_expand(&area);
    window.add(&area);

    let pixbuf = unsafe { gdk_pixbuf::Pixbuf::new(0, false, 8, 50, 50) }.unwrap();
    let pixels = unsafe { pixbuf.get_pixels() };
    let row_size = pixbuf.get_rowstride() as usize;
    for y in 0..50 {
        for x in 0..25 {
            pixels[row_size * y + 3 * x] = 255;
            pixels[row_size * y + 3 * x + 1] = 0;
            pixels[row_size * y + 3 * x + 2] = 0;
        }
        for x in 25..50 {
            pixels[row_size * y + 3 * x] = 0;
            pixels[row_size * y + 3 * x + 1] = 255;
            pixels[row_size * y + 3 * x + 2] = 0;
        }
    }

    let pixbuf_clone = pixbuf.clone();
    area.connect_draw(move |ref area, ref cr| {
        let width = area.get_allocated_width() as f64;
        let height = area.get_allocated_height() as f64;
//        let surface = gdk::cairo_sur...
//        cr.set_source_surface(&surface, 0, 0);
        cr.set_source_pixbuf(&pixbuf_clone, 0.0, 0.0);
        cr.rectangle(0.0, 0.0, width, height);
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

    let (gui_events_tx, gui_events_rx) = mpsc::channel();
    let (protocol_events_tx, protocol_events_rx) = mpsc::channel();
    unsafe {
        CONNECTION_OUT = Some(gui_events_tx);
    }
    unsafe {
        GTK_CONTEXT = Some(GtkContext {
            connection_in: protocol_events_rx,
            window: window.clone(),
            drawing_area: area.clone(),
        });
    }
    let view = GtkView {
        events_in: gui_events_rx,
        events_out: protocol_events_tx
    };
    std::thread::spawn(move || {
        let main_result = socket_thread_main(&args[1], view);

        if let Err(error_message) = main_result {
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

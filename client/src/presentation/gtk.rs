use ::{GuiEvent,ProtocolEvent,socket_thread_main,View,ConnectionConfig,
       MainError,FbSize,EncodingQuality,FbSlice};

use gtk;
use gdk;
use std;
use glib;
use cairo;
use gtk::WidgetExt;
use gtk::WindowExt;
use gtk::ContainerExt;
use gtk::DialogExt;
use gdk::{DisplayExt,SeatExt,DeviceExt};
use gdk::WindowExt as GdkWindowExt;

use gdk::prelude::ContextExt;

use gdk_pixbuf::Pixbuf;

use std::sync::mpsc;
use std::ops::Deref;

const KEY_F1 : u32 = 0xffbe;
const KEY_F2 : u32 = 0xffbf;
const KEY_F3 : u32 = 0xffc0;
const KEY_F4 : u32 = 0xffc1;
const KEY_F5 : u32 = 0xffc2;
const KEY_F6 : u32 = 0xffc3;
const KEY_F8 : u32 = 0xffc5;
const KEY_F11 : u32 = 0xffc8;

struct GtkContext {
    connection_in : mpsc::Receiver<ProtocolEvent>,
    window : gtk::Window,
    drawing_area : gtk::DrawingArea,
    pixbuf : Pixbuf,
    relative_mouse_mode : bool,
    f8_pressed : bool,
    fullscreen : bool
}
static mut GTK_CONTEXT : Option<GtkContext> = None;
fn gtk_context() -> &'static mut GtkContext {
    unsafe { GTK_CONTEXT.as_mut().unwrap() }
}
static mut CONNECTION_OUT : Option<mpsc::Sender<GuiEvent>> = None;
fn connection_out() -> &'static mpsc::Sender<GuiEvent> {
    unsafe { CONNECTION_OUT.as_ref().unwrap() }
}

fn handle_protocol_event(event : ProtocolEvent) {
    let context = gtk_context();
    let drawing_area = &context.drawing_area;
    match event {
        ProtocolEvent::ChangeDisplaySize(size) => {
            context.window.resize(size.width as i32,
                                  size.height as i32);
        },
        ProtocolEvent::UpdateFramebuffer(new_fb) => {
//            eprintln!("updating GUI framebuffer {:?}", ::std::time::Instant::now());
            let new_fb = new_fb.read().unwrap();
            let fb_copy = new_fb.data().clone();
            let size = new_fb.size();
            let pixbuf = Pixbuf::new_from_vec(fb_copy, 0, false, 8, 
                                              size.width as i32,
                                              size.height as i32,
                                              3 * (size.width as i32)); //TODO use constant for 3
            context.pixbuf = pixbuf;
            drawing_area.queue_draw();
//            eprintln!("updated GUI framebuffer {:?}", ::std::time::Instant::now());
        },
        ProtocolEvent::SetTitle(title) => {
            context.window.set_title(&format!("{} — flashvnc", title));
        }
    }
}
struct GtkView {
    events_in : Option<mpsc::Receiver<GuiEvent>>,
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
    fn get_events(&mut self) -> mpsc::Receiver<GuiEvent> {
        self.events_in.take().unwrap()
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

fn draw_menu(context : &'static mut GtkContext, cr : &cairo::Context,
             width : f64, _height : f64) {
    let item_width = width * 0.9;
    let item_height = 35.0;
    let item_spacing = 40.0;

    for (i, &(text, on)) in [
        ("F1: Encoding: Lossy, high quality", None),
        ("F2: Encoding: Lossy, medium quality", None),
        ("F3: Encoding: Lossy, medium, with interframe comparison", None),
        ("F4: Encoding: Lossy, low quality", None),
        ("F5: Encoding: Lossless", None),
        ("F6: Relative mouse mode", Some(context.relative_mouse_mode)),
        ("F11: Fullscreen", Some(context.fullscreen))
    ].iter().enumerate() {
        let y = (i as f64) * item_spacing;
        cr.set_source_rgb(0.3, 0.6, 1.0);
        cr.rectangle(0.0, y, item_width, item_height);
        cr.fill();

        cr.set_source_rgb(0.0, 0.0, 0.0);
        cr.set_font_size(15.0);
        cr.select_font_face("Sans", cairo::FontSlant::Normal, 
                            cairo::FontWeight::Normal);
        cr.move_to(5.0, y + item_spacing / 2.0);

        if let Some(on) = on {
            cr.show_text(&format!("[{}] {}",
                                  if on {
                                      "x"
                                  } else {
                                      " "
                                  }, text));
        } else {
            cr.show_text(text);
        }
    }
}

#[derive(PartialEq)]
enum ButtonState {
    Up,
    Down
}

trait PointerEvent : Deref<Target = gdk::Event> {
    fn get_position(&self) -> (f64, f64);
    fn get_state(&self) -> gdk::ModifierType;
    fn changed_button(&self) -> Option<(u32, ButtonState)>;
    fn is_scroll(&self) -> bool;
}

impl PointerEvent for gdk::EventButton {
    fn get_position(&self) -> (f64, f64) {
        self.get_position()
    }
    fn get_state(&self) -> gdk::ModifierType {
        self.get_state()
    }
    fn changed_button(&self) -> Option<(u32, ButtonState)> {
    //TODO x ignore Double/TripleButtonPress
        let state = if self.get_event_type() == gdk::EventType::ButtonRelease {
            ButtonState::Up
        } else {
            ButtonState::Down
        };
        Some((self.get_button(), state))
    }
    fn is_scroll(&self) -> bool {
        false
    }
}
impl PointerEvent for gdk::EventMotion {
    fn get_position(&self) -> (f64, f64) {
        self.get_position()
    }
    fn get_state(&self) -> gdk::ModifierType {
        self.get_state()
    }
    fn changed_button(&self) -> Option<(u32, ButtonState)> {
        None
    }
    fn is_scroll(&self) -> bool {
        false
    }
}
impl PointerEvent for gdk::EventScroll {
    fn get_position(&self) -> (f64, f64) {
        self.get_position()
    }
    fn get_state(&self) -> gdk::ModifierType {
        self.get_state()
    }
    fn changed_button(&self) -> Option<(u32, ButtonState)> {
        Some((if self.get_direction() == gdk::ScrollDirection::Down {
            5
        } else {
            4
        }, ButtonState::Down))
    }
    fn is_scroll(&self) -> bool {
        true
    }
}

fn warp_cursor_to((screen_x, screen_y) : (i32, i32)) {
    let display = gdk::Display::get_default().unwrap();
    let seat = display.get_default_seat().unwrap();
    let mouse = seat.get_pointer().unwrap();
    mouse.warp(&display.get_default_screen(), screen_x, screen_y);
}

fn warp_cursor_to_center(widget : &gtk::DrawingArea) {
    let center_x = widget.get_allocated_width() / 2;
    let center_y = widget.get_allocated_height() / 2;
    warp_cursor_to(widget.get_window().unwrap().get_root_coords(
            center_x, center_y));
}

fn compute_buttons_state<E>(e : &E) -> (u8, u8)
    where E : PointerEvent 
{
    let gdk_state = e.get_state();
    let mut buttons_state = 0u8;
    for (i, &mask) in [
        gdk::BUTTON1_MASK,
        gdk::BUTTON2_MASK,
        gdk::BUTTON3_MASK,
        gdk::BUTTON4_MASK,
        gdk::BUTTON5_MASK].iter().enumerate()
    {
        if gdk_state.contains(mask) {
            buttons_state |= 1 << i;
        }
    }

    let original_buttons_state = buttons_state;

    if let Some((button, state)) = e.changed_button() {
        if state == ButtonState::Down {
            buttons_state |= 1 << (button - 1);
        }
        if state == ButtonState::Up {
            buttons_state &= !(1 << (button - 1));
        }
    }

    (original_buttons_state, buttons_state)
}

fn handle_mouse_input<E>(widget : &gtk::DrawingArea, e : &E) -> gtk::Inhibit 
    where E : PointerEvent
{
    let (x, y) = e.get_position();

    let (buttons_state_before_event, buttons_state) = compute_buttons_state(e);
    let buttons_state = if e.is_scroll() {
        buttons_state_before_event
    } else {
        buttons_state
    };

    if gtk_context().relative_mouse_mode {
        let center_x = widget.get_allocated_width() / 2;
        let center_y = widget.get_allocated_height() / 2;
        let center_x = center_x as f64;
        let center_y = center_y as f64;
        let dx = x - center_x;
        let dy = y - center_y;
        if dx != 0.0 || dy != 0.0 || e.changed_button().is_some() {
            connection_out().send(
                GuiEvent::RelativePointer {
                    state: buttons_state,
                    dx: x - center_x,
                    dy: y - center_y
                }).unwrap_or(());
            warp_cursor_to_center(widget);
        }
    } else {
        connection_out().send(
            GuiEvent::Pointer {
                state: buttons_state,
                x: x as i32,
                y: y as i32
            }).unwrap_or(());
    }

    gtk::Inhibit(true)
}

fn handle_keyboard_input(widget : &gtk::DrawingArea, e : &gdk::EventKey) 
    -> gtk::Inhibit 
{
    let context = gtk_context();
    let key = e.get_keyval();
    let press = e.get_event_type() == gdk::EventType::KeyPress;

    if press {
        let f8_pressed_now = key == KEY_F8;
        let f8_was_pressed = context.f8_pressed;
        if f8_was_pressed {
            match key {
                KEY_F1 => {
                    connection_out().send(GuiEvent::SetEncodingQuality(
                            EncodingQuality::LossyHigh)).unwrap_or(());
                },
                KEY_F2 => {
                    connection_out().send(GuiEvent::SetEncodingQuality(
                            EncodingQuality::LossyMedium)).unwrap_or(());
                },
                KEY_F3 => {
                    connection_out().send(GuiEvent::SetEncodingQuality(
                            EncodingQuality::LossyMediumInterframeComparison))
                        .unwrap_or(());
                },
                KEY_F4 => {
                    connection_out().send(GuiEvent::SetEncodingQuality(
                            EncodingQuality::LossyLow)).unwrap_or(());
                },
                KEY_F5 => {
                    connection_out().send(GuiEvent::SetEncodingQuality(
                            EncodingQuality::Lossless)).unwrap_or(());
                },
                KEY_F6 => {
                    context.relative_mouse_mode = !context.relative_mouse_mode;
                    if context.relative_mouse_mode {
                        warp_cursor_to_center(widget);
                    }
                },
                KEY_F11 => {
                    context.fullscreen = !context.fullscreen;
                    if context.fullscreen {
                        context.window.fullscreen();
                    } else {
                        context.window.unfullscreen();
                    }
                },
                _ => { }
            }
            context.f8_pressed = false;
        }
        if f8_pressed_now {
            context.f8_pressed = true;
        }

        if f8_was_pressed || f8_pressed_now {
            context.drawing_area.queue_draw();
            return gtk::Inhibit(true);
        }
    }

    connection_out().send(
        GuiEvent::Keyboard {
            key: key,
            down: press
        }).unwrap_or(());
    //eprintln!("keyboard event: {}", e.get_keyval());
    gtk::Inhibit(true)
}

fn handle_resize_event(e : &gdk::EventConfigure) -> bool {
    let (width, height) = e.get_size();
    let size = FbSize::new(width as usize, height as usize);
    connection_out().send(GuiEvent::Resized(size)).unwrap_or(());
    false
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
    area.connect_draw(move |ref area, ref cr| {
        let width = area.get_allocated_width() as f64;
        let height = area.get_allocated_height() as f64;
//        let surface = gdk::cairo_sur...
//        cr.set_source_surface(&surface, 0, 0);
//        let pixbuf = gdk_pixbuf::Pixbuf::new_from_file("/tmp/flashvnc-still-image/reference.png").unwrap();
//        cr.set_source_pixbuf(&pixbuf, 0.0, 0.0);

//        cr.set_source_pixbuf(&pixbuf_clone, 0.0, 0.0);
        let context = gtk_context();
        cr.set_source_pixbuf(&context.pixbuf, 0.0, 0.0);
        cr.rectangle(0.0, 0.0, width, height);
        cr.fill();

        if context.f8_pressed {
            draw_menu(context, cr, width, height);
        }

//        eprintln!("drawing GUI framebuffer {:?}", ::std::time::Instant::now());
        gtk::Inhibit(true)
    });

    area.connect_button_press_event(|ref widget, e| {
        handle_mouse_input(widget, e)
    });
    area.connect_button_release_event(|ref widget, e| {
        handle_mouse_input(widget, e)
    });
    area.connect_motion_notify_event(|ref widget, e| {
        handle_mouse_input(widget, e)
    });
    area.connect_scroll_event(|ref widget, e| {
        handle_mouse_input(widget, e)
    });
    area.connect_key_press_event(|ref widget, ref e| {
        handle_keyboard_input(widget, e)
    });
    area.connect_key_release_event(|ref widget, ref e| {
        handle_keyboard_input(widget, e)
    });

    area.connect_configure_event(|ref _widget, ref e| {
        handle_resize_event(e)
    });
    let mut event_mask = gdk::EventMask::from_bits_truncate(
        area.get_events() as u32);
    event_mask.insert(gdk::BUTTON_PRESS_MASK);
    event_mask.insert(gdk::BUTTON_RELEASE_MASK);
    event_mask.insert(gdk::POINTER_MOTION_MASK);
    event_mask.insert(gdk::SCROLL_MASK);
    event_mask.insert(gdk::KEY_PRESS_MASK);
    event_mask.insert(gdk::KEY_RELEASE_MASK);
    event_mask.insert(gdk::STRUCTURE_MASK);
    area.set_events(event_mask.bits() as i32);

    area.set_can_focus(true);

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
            pixbuf: Pixbuf::new_from_vec(
                    one_pixel_fb, 0, false, 8, 1, 1, 3), //TODO use constant
            relative_mouse_mode: false,
            f8_pressed: false,
            fullscreen: false
        });
    }
    let view = GtkView {
        events_in: Some(gui_events_rx),
        events_out: protocol_events_tx
    };
    std::thread::spawn(move || {
        let main_result = socket_thread_main(config, view);

        if let Err(MainError(error_message)) = main_result {
            show_fatal_error(error_message);
        }
        //TODO call main_quit
    });

    window.resize(534, 600);

    window.set_title("flashvnc");
    //window.fullscreen();

    window.connect_delete_event(|_, _| {
        gtk::main_quit();
        gtk::Inhibit(false)
    });
    window.show_all();
    gtk::main();
}

use ::{GuiEvent,ProtocolEvent,socket_thread_main,View,ConnectionConfig,
       MainError,FbSize,EncodingQuality,ViewOutput,PixelFormat};
use presentation::menu::{Menu,MenuActionHandler,DrawingContext};

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

use std::sync::{mpsc,Arc,Mutex};
use std::ops::Deref;

const COLORSPACE_RGB : i32 = 0;

struct GtkContext {
    connection_in : mpsc::Receiver<ProtocolEvent>,
    window : gtk::Window,
    drawing_area : gtk::DrawingArea,
    pixbuf : Pixbuf,
    menu : Menu<GtkMenuActionHandler>,
    current_size : Option<FbSize>,
    fb_updated_tx : mpsc::Sender<()>,
    fb_updated : bool
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
            context.current_size = Some(size);
        },
        ProtocolEvent::UpdateFramebuffer(rgb, size) => {
//            eprintln!("updating GUI framebuffer {:?}", ::std::time::Instant::now());

//            use libc::{rand,RAND_MAX};
//            use std::time::Duration;
//            let r_1 = unsafe { rand() } as f64 / RAND_MAX as f64;
//            let r_2 = unsafe { rand() } as f64 / RAND_MAX as f64;
//            if r_1 > 0.995 {
//                std::thread::sleep(Duration::from_millis(
//                        (50.0 + r_2 * 25.0) as u64));
//            } else {
//                std::thread::sleep(Duration::from_millis(
//                        (1.0 + r_2 * 28.0) as u64));
//            }

            let stride = size.width * 3;
            let pixbuf = Pixbuf::new_from_vec(rgb, COLORSPACE_RGB,
                                              false, 8, 
                                              size.width as i32,
                                              size.height as i32,
                                              stride as i32);
            context.pixbuf = pixbuf;
            context.fb_updated = true;
            drawing_area.queue_draw();
//            eprintln!("updated GUI framebuffer {:?}", ::std::time::Instant::now());
        },
        ProtocolEvent::UpdateCursor(rgba, size, hotspot) => {
            let window = drawing_area.get_window().unwrap();
            if size.0 > 0 {
                let display = gdk::Display::get_default().unwrap();
                let pixbuf = Pixbuf::new_from_vec(
                    rgba, COLORSPACE_RGB, true, 8,
                    size.0 as i32, size.1 as i32,
                    (4 * size.0) as i32);
                let cursor = gdk::Cursor::new_from_pixbuf(
                    &display, &pixbuf, 
                    hotspot.0 as i32, hotspot.1 as i32);

                window.set_cursor(Some(&cursor));
            } else {
                window.set_cursor(None);
            }
        },
        ProtocolEvent::SetTitle(title) => {
            context.window.set_title(&format!("{} — flashvnc", title));
        }
    }
}
struct GtkView {
    events_in : Option<mpsc::Receiver<GuiEvent>>,
    output : GtkViewOutput
}
impl View for GtkView {
    type Output = GtkViewOutput;

    fn get_events(&mut self) -> mpsc::Receiver<GuiEvent> {
        self.events_in.take().unwrap()
    }
    fn get_output(&self) -> &GtkViewOutput {
        &self.output
    }
    fn desired_pixel_format() -> PixelFormat {
        PixelFormat::Rgb
    }
}
#[derive(Clone)]
struct GtkViewOutput {
    events_out : mpsc::SyncSender<ProtocolEvent>,
    fb_updated_receiver : Arc<Mutex<mpsc::Receiver<()>>>
}
impl ViewOutput for GtkViewOutput {
    fn handle_event(&self, event : ProtocolEvent) {
        self.events_out.send(event).unwrap();
        glib::idle_add(|| {
            let mut i = 0;
            while let Ok(event) = gtk_context().connection_in.try_recv() {
                handle_protocol_event(event);
                i += 1;
                if i >= 1 {
                    break;
                }
            }
            glib::Continue(false)
        });
    }

    fn update_framebuffer_sync(&self, fb_data : Vec<u8>, size : FbSize) {
        self.update_framebuffer(fb_data, size);
        self.fb_updated_receiver.lock().unwrap().recv().unwrap();
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

struct GtkMenuActionHandler;
impl MenuActionHandler for GtkMenuActionHandler {
    fn set_encoding_quality(&mut self, quality : EncodingQuality) {
        connection_out().send(GuiEvent::SetEncodingQuality(
                quality)).unwrap_or(());
    }
    fn set_fullscreen(&mut self) {
        gtk_context().window.fullscreen();
    }
    fn unset_fullscreen(&mut self) {
        gtk_context().window.unfullscreen();
    }
    fn start_relative_mouse_mode(&mut self) {
        warp_cursor_to_center(&gtk_context().drawing_area);
    }
    fn stop_relative_mouse_mode(&mut self) { }
}

struct CairoContext<'a>(&'a cairo::Context);
impl<'a> DrawingContext for CairoContext<'a> {
    fn fill_background_rect(&mut self, x : f64, y : f64, w : f64, h : f64) {
        self.0.set_source_rgb(0.3, 0.6, 1.0);
        self.0.rectangle(x, y, w, h);
        self.0.fill();
    }
    fn draw_text(&mut self, x : f64, y : f64, text : &str) {
        self.0.set_source_rgb(0.0, 0.0, 0.0);
        self.0.set_font_size(15.0);
        self.0.select_font_face("Sans", cairo::FontSlant::Normal, 
                                cairo::FontWeight::Normal);
        self.0.move_to(x, y);
        self.0.show_text(text);
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

fn compute_buttons_state<E>(e : &E) -> u8
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

    if let Some((button, state)) = e.changed_button() {
        if state == ButtonState::Down {
            buttons_state |= 1 << (button - 1);
        }
        if state == ButtonState::Up {
            buttons_state &= !(1 << (button - 1));
        }
    }

    buttons_state
}

fn handle_mouse_input<E>(widget : &gtk::DrawingArea, e : &E) -> gtk::Inhibit 
    where E : PointerEvent
{
    let (x, y) = e.get_position();

    let buttons_state = compute_buttons_state(e);
    let buttons_without_scrolling = buttons_state & !0x18;

    if gtk_context().menu.relative_mouse_mode() {
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
        if e.is_scroll() {
            connection_out().send(
                GuiEvent::RelativePointer {
                    state: buttons_without_scrolling,
                    dx: 0.0,
                    dy: 0.0
                }).unwrap_or(());
        }
    } else {
        connection_out().send(
            GuiEvent::Pointer {
                state: buttons_state,
                x: x as i32,
                y: y as i32
            }).unwrap_or(());
        if e.is_scroll() {
            connection_out().send(
                GuiEvent::Pointer {
                    state: buttons_without_scrolling,
                    x: x as i32,
                    y: y as i32
                }).unwrap_or(());
        }
    }

    gtk::Inhibit(true)
}

fn handle_keyboard_input(_widget : &gtk::DrawingArea, e : &gdk::EventKey) 
    -> gtk::Inhibit 
{
    let context = gtk_context();
    let key = e.get_keyval();
    let press = e.get_event_type() == gdk::EventType::KeyPress;

    if press && context.menu.intercept_key_press(key) {
        context.drawing_area.queue_draw();
        return gtk::Inhibit(true);
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
    let context = gtk_context();
    if context.current_size.is_none() || context.current_size == Some(size) {
        return false;
    }
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

        if context.menu.visible() {
            context.menu.draw(&mut CairoContext(cr), width, height);
        }

        if context.fb_updated {
            context.fb_updated = false;
            context.fb_updated_tx.send(()).unwrap_or(());
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
    let (protocol_events_tx, protocol_events_rx) = mpsc::sync_channel(4);
    let (fb_updated_tx, fb_updated_rx) = mpsc::channel();
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
                    one_pixel_fb, COLORSPACE_RGB, false, 8, 1, 1, 3),
            menu: Menu::new(GtkMenuActionHandler { }),
            current_size: None,
            fb_updated_tx: fb_updated_tx,
            fb_updated: false
        });
    }
    let view = GtkView {
        events_in: Some(gui_events_rx),
        output: GtkViewOutput {
            events_out: protocol_events_tx,
            fb_updated_receiver: Arc::new(Mutex::new(fb_updated_rx))
        }
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

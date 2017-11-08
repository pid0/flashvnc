use ::{GuiEvent,ProtocolEvent,socket_thread_main,View,ConnectionConfig,
       MainError,FbSize,EncodingQuality,ViewOutput,PixelFormat};
use presentation::menu::{MenuActionHandler,DrawingContext};
use presentation::menu::Menu as BaseMenu;
type Menu = BaseMenu<SdlMenuActionHandler>;

use sdl2;
use sdl2::event::{Event,WindowEvent};
use sdl2::{EventPump,EventSubsystem};
use sdl2::pixels::{Color,PixelFormatEnum};
use sdl2::surface::{Surface,SurfaceRef};
use sdl2::render::BlendMode;
use sdl2::video::{Window,FullscreenType};
use sdl2::mouse::{MouseUtil,MouseState,Cursor,MouseWheelDirection,MouseButton};
use sdl2::keyboard::{self,Keycode};
use sdl2::keyboard::Mod as KeyMod;
use sdl2::rect::Rect;
use sdl2::messagebox::{MESSAGEBOX_ERROR,show_simple_message_box};

use std;
use std::sync::{mpsc,Arc,Mutex};
use std::rc::Rc;
use std::cell::RefCell;
use std::ops::Range;

use presentation::x11_keysyms;

const ASCII_TEXT_RANGE : Range<u32> = 0x20..0x100;
const ASCII_DEL : u32 = 127;
struct KeysymTextRange;
impl KeysymTextRange {
    pub fn contains(&self, keysym : u32) -> bool {
        ASCII_TEXT_RANGE.contains(keysym) && keysym != ASCII_DEL
    }
}
const KEYSYM_TEXT_RANGE : KeysymTextRange = KeysymTextRange{ };

fn is_little_endian() -> bool {
    let n : u32 = 1;
    let byte = &n as *const u32 as *const u8;
    unsafe { *byte == 1 }
}

fn pixel_format_rgba() -> PixelFormatEnum {
    if is_little_endian() {
        PixelFormatEnum::ABGR8888
    } else {
        PixelFormatEnum::RGBA8888
    }
}

fn ctrl_pressed(key_mod : &KeyMod) -> bool {
    key_mod.contains(keyboard::LCTRLMOD)
        || key_mod.contains(keyboard::RCTRLMOD)
}
fn shift_pressed(key_mod : &KeyMod) -> bool {
    key_mod.contains(keyboard::LSHIFTMOD)
        || key_mod.contains(keyboard::RSHIFTMOD)
}

fn get_buttons_state(mouse : MouseState, scroll_y : i32) -> u8 {
    let mut buttons_state = 0u8;
    for (i, &pressed) in [
        mouse.left(),
        mouse.middle(),
        mouse.right(),
        scroll_y > 0,
        scroll_y < 0].iter().enumerate()
    {
        if pressed {
            buttons_state |= 1 << i;
        }
    }
    buttons_state
}

fn updated_buttons_state(buttons_state : u8, button : MouseButton) -> u8 {
    match button {
        MouseButton::Left => buttons_state | 0x1,
        MouseButton::Middle => buttons_state | 0x2,
        MouseButton::Right => buttons_state | 0x4,
        _ => buttons_state
    }
}

fn handle_protocol_event(
    window : &mut Window, 
    events : &EventPump,
    event : ProtocolEvent,
    menu : &Menu,
    cursor : &mut Option<Cursor>,
    fb_updated_tx : &mpsc::Sender<()>) 
{
    match event {
        ProtocolEvent::ChangeDisplaySize(size) => {
            window.set_size(size.width as u32, size.height as u32).unwrap();
        },
        ProtocolEvent::UpdateFramebuffer(mut bgrx, size) => {
            let mut surface = window.surface(events).unwrap();
            let mut image = Surface::from_data(
                &mut bgrx[..],
                size.width as u32,
                size.height as u32,
                (size.width * 4) as u32,
                PixelFormatEnum::RGB888).unwrap();
            image.set_blend_mode(BlendMode::None).unwrap();
            //TODO if you want, you can set SDL_HINT_FRAMEBUFFER_ACCELERATION
            //note: the window surface becomes invalid when the window is resized
            //surface.fill_rect(None, Color::RGB(255, 0, 0)).unwrap();
            image.blit(None, &mut surface, None).unwrap(); 

            if menu.visible() {
                menu.draw(
                    &mut SdlSurface(&mut surface),
                    size.width as f64,
                    size.height as f64);
            }
            surface.finish().unwrap();

            fb_updated_tx.send(()).unwrap_or(());
        },
        ProtocolEvent::UpdateCursor(mut rgba, size, hotspot) => {
            if size.0 > 0 {
                let cursor_image = Surface::from_data(
                    &mut rgba[..],
                    size.0 as u32,
                    size.1 as u32,
                    (size.0 * 4) as u32,
                    pixel_format_rgba()).unwrap();
                *cursor = Some(Cursor::from_surface(
                        cursor_image, 
                        hotspot.0 as i32, hotspot.1 as i32).unwrap());
                cursor.as_ref().unwrap().set();
            }
        },
        ProtocolEvent::SetTitle(title) => {
            //TODO ‘—’ does not work
            //window.set_title(&format!("{} — flashvnc", title)).unwrap();
            window.set_title(&format!("{} --- flashvnc", title)).unwrap();
        },
    }
}

struct SdlMenuActionHandler {
    window : Rc<RefCell<Window>>,
    gui_events_tx : mpsc::Sender<GuiEvent>,
    mouse : MouseUtil
}
impl MenuActionHandler for SdlMenuActionHandler {
    fn set_encoding_quality(&mut self, quality : EncodingQuality) {
        self.gui_events_tx.send(GuiEvent::SetEncodingQuality(
                quality)).unwrap_or(());
    }
    fn set_fullscreen(&mut self) {
        self.window.borrow_mut().set_fullscreen(FullscreenType::True).unwrap();
    }
    fn unset_fullscreen(&mut self) {
        self.window.borrow_mut().set_fullscreen(FullscreenType::Off).unwrap();
    }
    fn start_relative_mouse_mode(&mut self) {
        self.mouse.set_relative_mouse_mode(true);
        //warp_cursor_to_center(&gtk_context().drawing_area);
    }
    fn stop_relative_mouse_mode(&mut self) {
        self.mouse.set_relative_mouse_mode(false);
    }
}

struct SdlSurface<'a>(&'a mut SurfaceRef);
impl<'a> DrawingContext for SdlSurface<'a> {
    fn fill_background_rect(&mut self, x : f64, y : f64, w : f64, h : f64) {
        self.0.fill_rect(
            Rect::new(x as i32, y as i32, w as u32,
                      h as u32), 
            Color::RGB(
                (0.3 * 255.0) as u8,
                (0.6 * 255.0) as u8,
                255)).unwrap();
    }
    fn draw_text(&mut self, _x : f64, _y : f64, _text : &str) {
//        self.0.set_source_rgb(0.0, 0.0, 0.0);
//        self.0.set_font_size(15.0);
//        self.0.select_font_face("Sans", cairo::FontSlant::Normal, 
//                                cairo::FontWeight::Normal);
//        self.0.move_to(x, y);
//        self.0.show_text(text);
    }
}

struct AssertSend<T> {
    t : T
}
impl<T> AssertSend<T> {
    fn new(t : T) -> Self {
        Self {
            t: t
        }
    }
}
impl<T> std::ops::Deref for AssertSend<T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.t
    }
}
unsafe impl<T> Send for AssertSend<T> { }

struct SdlView {
    events_in : Option<mpsc::Receiver<GuiEvent>>,
    output : SdlViewOutput
}
impl View for SdlView {
    type Output = SdlViewOutput;

    fn get_events(&mut self) -> mpsc::Receiver<GuiEvent> {
        self.events_in.take().unwrap()
    }
    fn get_output(&self) -> &SdlViewOutput {
        &self.output
    }
    fn desired_pixel_format() -> PixelFormat {
        PixelFormat::NativeBgrx
    }
}
#[derive(Clone)]
struct SdlViewOutput {
    events_out : mpsc::SyncSender<ProtocolEvent>,
    event_sys : Arc<AssertSend<EventSubsystem>>,
    fb_updated_rx : Arc<Mutex<mpsc::Receiver<()>>>
}
impl SdlViewOutput {
    fn wake_up_main_loop(&self) {
        self.event_sys.push_custom_event(()).unwrap();
    }
}
impl ViewOutput for SdlViewOutput {
    fn handle_event(&self, event : ProtocolEvent) {
        self.events_out.send(event).unwrap_or(());
        self.wake_up_main_loop();
    }
    fn update_framebuffer_sync(&self, fb_data : Vec<u8>, size : FbSize) {
        self.update_framebuffer(fb_data, size);
        self.fb_updated_rx.lock().unwrap().recv().unwrap_or(());
    }
}

fn show_fatal_error(error_string : String) {
    let message = format!("Connection closed due to an error:\n{}",
                            error_string);
    eprintln!("{}", message);
    show_simple_message_box(
        MESSAGEBOX_ERROR,
        "Connection closed",
        &message[..],
        None).unwrap();
}

fn sdl_keycode_to_x11_keysym(keycode : Keycode, key_mod : KeyMod) -> u32 {
    use self::Keycode::*;
    use self::x11_keysyms::*;
    let keycode_num = keycode as i32 as u32;

    match keycode {
        Backspace => XK_BackSpace,
        Tab => XK_Tab,
        Clear => XK_Clear,
        Return => XK_Return,
        Pause => XK_Pause,
        Escape => XK_Escape,
        Space => XK_Space,
        Delete => XK_Delete,
        Kp0 => XK_KP_0,
        Kp1 => XK_KP_1,
        Kp2 => XK_KP_2,
        Kp3 => XK_KP_3,
        Kp4 => XK_KP_4,
        Kp5 => XK_KP_5,
        Kp6 => XK_KP_6,
        Kp7 => XK_KP_7,
        Kp8 => XK_KP_8,
        Kp9 => XK_KP_9,
        KpPeriod => XK_KP_Decimal,
        KpDivide => XK_KP_Divide,
        KpMultiply => XK_KP_Multiply,
        KpMinus => XK_KP_Subtract,
        KpPlus => XK_KP_Add,
        KpEnter => XK_KP_Enter,
        KpEquals => XK_KP_Equal,
        Up => XK_Up,
        Down => XK_Down,
        Right => XK_Right,
        Left => XK_Left,
        Insert => XK_Insert,
        Home => XK_Home,
        End => XK_End,
        PageUp => XK_Page_Up,
        PageDown => XK_Page_Down,
        F1 => XK_F1,
        F2 => XK_F2,
        F3 => XK_F3,
        F4 => XK_F4,
        F5 => XK_F5,
        F6 => XK_F6,
        F7 => XK_F7,
        F8 => XK_F8,
        F9 => XK_F9,
        F10 => XK_F10,
        F11 => XK_F11,
        F12 => XK_F12,
        F13 => XK_F13,
        F14 => XK_F14,
        F15 => XK_F15,
        NumLockClear => XK_Num_Lock,
        CapsLock => XK_Caps_Lock,
        ScrollLock => XK_Scroll_Lock,
        RShift => XK_Shift_R,
        LShift => XK_Shift_L,
        RCtrl => XK_Control_R,
        LCtrl => XK_Control_L,
        RAlt => XK_Alt_R,
        LAlt => XK_Alt_L,
//        RMeta => XK_Meta_R,
//        LMeta => XK_Meta_L,
        LGui => XK_Super_L,
        RGui => XK_Super_R,
        Application => XK_Multi_key,
        Mode => XK_Mode_switch,
        Help => XK_Help,
        PrintScreen => XK_Print,
        Sysreq => XK_Sys_Req,
        _ if shift_pressed(&key_mod)
            && keycode_num >= 'a' as u32 && keycode_num <= 'z' as u32 =>
        {
            keycode_num - 0x20
        },
//        _ if key_mod.contains(keyboard::LSHIFTMOD) 
//            || key_mod.contains(keyboard::RSHIFTMOD) => {
//            //TODO need some lib for this (must be layout dependent)
//            match keycode_num {
//                n if n >= '1' as u32 && n <= '9' as u32 => keycode_num & !0x10,
//                n if n >= 'a' as u32 && n <= 'z' as u32 => keycode_num & !0x20,
//                n if n == '`' as u32 => '~' as u32,
//                n if n >= '[' as u32 || n <= ']' as u32 => keycode_num | 0x10,
//                n if n == '-' as u32 => '_' as u32,
//                _ => keycode_num
//            }
//        },
        _ => keycode_num
	}
}

struct MainLoop {
    window : Rc<RefCell<Window>>,
    events : EventPump,
    pressed_text_keysyms : Vec<u32>,
    menu : Menu,
    protocol_events_rx: mpsc::Receiver<ProtocolEvent>,
    gui_events_tx: mpsc::Sender<GuiEvent>,
    fb_updated_tx : mpsc::Sender<()>,
    cursor : Option<Cursor>,
}
impl MainLoop {
    pub fn iterate(&mut self) -> bool {
        let event = self.events.wait_event();

        match event {
            Event::Quit {..} => {
                return false;
            },
            Event::MouseMotion {..}
            | Event::MouseButtonDown {..}
            | Event::MouseButtonUp {..}
            | Event::MouseWheel{..} => {
                let scroll_y = match event {
                    Event::MouseWheel { y, 
                        direction: MouseWheelDirection::Normal, .. } => y,
                    Event::MouseWheel { y, 
                        direction: MouseWheelDirection::Flipped, .. } => -y,
                    _ => 0
                };

                let mouse = self.events.mouse_state();
                let mut buttons_state = get_buttons_state(mouse, scroll_y);
                if let Event::MouseButtonDown { mouse_btn, .. } = event {
                    buttons_state = updated_buttons_state(
                        buttons_state, mouse_btn);
                }
                let buttons_state_without_scrolling = buttons_state & !0x18;

                if self.menu.relative_mouse_mode() {
                    let relative_state = self.events.relative_mouse_state();
                    let (dx, dy) = (relative_state.x(), relative_state.y());
                    self.gui_events_tx.send(GuiEvent::RelativePointer {
                        state: buttons_state,
                        dx: dx as f64,
                        dy: dy as f64
                    }).unwrap_or(());

                    if scroll_y != 0 {
                        self.gui_events_tx.send(GuiEvent::RelativePointer {
                            state: buttons_state_without_scrolling,
                            dx: 0.0,
                            dy: 0.0
                        }).unwrap_or(());
                    }
                } else {
                    self.gui_events_tx.send(GuiEvent::Pointer {
                        state: buttons_state,
                        x: mouse.x(),
                        y: mouse.y()
                    }).unwrap_or(());

                    if scroll_y != 0 {
                        self.gui_events_tx.send(GuiEvent::Pointer {
                            state: buttons_state_without_scrolling,
                            x: mouse.x(),
                            y: mouse.y()
                        }).unwrap_or(());
                    }
                }
            },

            //TODO keypad is sent twice when off
            Event::KeyDown { keycode: Some(keycode), repeat: _, keymod, .. }
            | Event::KeyUp { keycode: Some(keycode), repeat: _, keymod, .. } => 
            {
                let press = if let Event::KeyDown {..} = event { 
                    true
                } else {
                    false
                };
                let keysym = sdl_keycode_to_x11_keysym(keycode, keymod);

                if ctrl_pressed(&keymod) 
                    || !KEYSYM_TEXT_RANGE.contains(keycode as i32 as u32)
                {
                    self.handle_key_event(keysym, press);
                }

                if !press {
                    let currently_pressed_keys : Vec<_> = self.events
                        .keyboard_state()
                        .pressed_scancodes().filter_map(Keycode::from_scancode)
                        .map(|k| k as i32 as u32)
                        .collect();
                    let released_keys : Vec<_> = self.pressed_text_keysyms
                        .drain_filter(|k| !currently_pressed_keys.contains(k))
                        .collect();
                    for previously_pressed_key in released_keys {
                        self.handle_key_event(previously_pressed_key, false);
                    }
                }
            },
            Event::TextInput { text, .. } => {
                let character = text.chars().next().unwrap();
                let keysym = character as u32;
                if KEYSYM_TEXT_RANGE.contains(keysym) {
                    self.pressed_text_keysyms.push(keysym);
                    self.handle_key_event(keysym, true);
                }
            },

            Event::Window { win_event: WindowEvent::Resized(w, h), .. } => {
                let size = FbSize::new(w as usize, h as usize);
                self.gui_events_tx.send(GuiEvent::Resized(size)).unwrap_or(());
            }

            _ => { }
        }

        if let Ok(event) = self.protocol_events_rx.try_recv() {
            handle_protocol_event(&mut self.window.borrow_mut(), 
                                  &self.events, event,
                                  &self.menu,
                                  &mut self.cursor,
                                  &self.fb_updated_tx);
        }

        true
    }

    fn handle_key_event(&mut self, keysym : u32, down : bool) {
        //eprintln!("got key: {} {}", keysym, press);

        if down && self.menu.intercept_key_press(keysym) {
            if self.menu.visible() {
                let window = self.window.borrow();
                let (w, h) = window.size();
                let mut surface = window.surface(&self.events).unwrap();
                self.menu.draw(
                    &mut SdlSurface(&mut surface),
                    w as f64,
                    h as f64);
                surface.finish().unwrap();
            }
            return;
        }

        self.gui_events_tx.send(GuiEvent::Keyboard {
            key: keysym,
            down: down
        }).unwrap_or(());
    }
}


pub fn run(config : ConnectionConfig) {
    let sdl_context = sdl2::init().unwrap();
    let video = sdl_context.video().unwrap();

    video.text_input().start();

    let window = video.window("flashvnc", 534, 600)
        .resizable()
        .build().unwrap();
    let events = sdl_context.event_pump().unwrap();

    //TODO constant endianness-dependent
    if window.surface(&events).unwrap().pixel_format_enum() 
        != PixelFormatEnum::RGB888 {
        eprintln!("{}",
                  "warning: native pixel format not bgrx, drawing might be \
                  slow");
    }
    let window = Rc::new(RefCell::new(window));

    let event_sys = sdl_context.event().unwrap();
    event_sys.register_custom_event::<()>().unwrap();

    let (gui_events_tx, gui_events_rx) = mpsc::channel();
    let (protocol_events_tx, protocol_events_rx) = mpsc::sync_channel(5);
    let (fb_updated_tx, fb_updated_rx) = mpsc::channel();
    let view = SdlView {
        events_in: Some(gui_events_rx),
        output: SdlViewOutput {
            events_out: protocol_events_tx,
            event_sys: Arc::new(AssertSend::new(event_sys)),
            fb_updated_rx: Arc::new(Mutex::new(fb_updated_rx))
        }
    };
    std::thread::spawn(move || {
        let main_result = socket_thread_main(config, view);

        if let Err(MainError(error_message)) = main_result {
            show_fatal_error(error_message);
            std::process::exit(1);
        }
        std::process::exit(0);
    });

    let menu = Menu::new(SdlMenuActionHandler {
        window: window.clone(),
        gui_events_tx: gui_events_tx.clone(),
        mouse: sdl_context.mouse()
    });

    let mut main_loop = MainLoop {
        window: window,
        events: events,
        pressed_text_keysyms: Vec::new(),
        menu: menu,
        protocol_events_rx: protocol_events_rx,
        gui_events_tx: gui_events_tx,
        fb_updated_tx: fb_updated_tx,
        cursor: None
    };

    while main_loop.iterate() { }
}

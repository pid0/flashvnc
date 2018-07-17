#![feature(conservative_impl_trait)]
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

#![feature(const_fn)]
#![feature(drop_types_in_const)]
#![feature(test)]
#![feature(range_contains)]
#![feature(drain_filter)]

#[cfg(test)]
extern crate test;

extern crate gtk;
extern crate gdk;
extern crate cairo;
extern crate glib;
extern crate gdk_pixbuf;
#[macro_use]
extern crate derivative;
extern crate flate2;
extern crate libc;
extern crate sdl2;

use std::io;
use std::io::{BufReader,BufWriter,Read,Write};
//use std::io::{Read,Write};
use std::net::{TcpStream,UdpSocket};
use std::sync::mpsc;
use std::collections::VecDeque;

pub mod infrastructure;
use infrastructure::thread_pool::{ThreadPool,Future,FutureCollection};
use infrastructure::ModeLock;

mod presentation;
use presentation::gtk as gtk_frontend;
use presentation::sdl as sdl_frontend;

pub mod protocol;

mod framebuffer;
pub use framebuffer::{Bgrx,FbSize,Framebuffer,FbSlice,FbAccess,
                      PixelFormat};
pub type SharedFb = Arc<ModeLock<Framebuffer>>;

mod encoding;
use encoding::{DecodingJob,DecodingMaster,EncodingMethod,TightData};
mod tight;

use protocol::rfb;
use protocol::parsing::io_input::SharedBuf;
use protocol::parsing::Packet;
use std::cell::RefCell;
use std::sync::{Arc,Mutex};
use std::time::{Duration,Instant};
use std::str::FromStr;

//same as TurboVNC uses
//TODO use one that is most suitable for this client
const PIXEL_FORMAT : rfb::PixelFormat = rfb::PixelFormat {
    bits_per_pixel: 32,
    depth: 24,

    big_endian: false,
    true_color: true,

    red_max: 255,
    green_max: 255,
    blue_max: 255,

    red_shift: 16,
    green_shift: 8,
    blue_shift: 0
};
const PIXEL_FORMAT_BYTES_PER_PIXEL : usize = 
    (PIXEL_FORMAT.bits_per_pixel as usize) / 8;
const TPIXEL_SIZE : usize = 3;
//[0, 0, 255, 0, 0, 0, 255, 0, 0, 0]
//-> 0x00ff0000 -> red: 0x00ff & 0xff

struct FbPixelFormat {
    bytes_per_pixel : usize
}
const FB_PIXEL_FORMAT : FbPixelFormat = FbPixelFormat {
    bytes_per_pixel: 4
};

//const CURSOR_BYTES_PER_PIXEL : usize = 4;

pub struct ConnectionConfig {
    pub host : String,
    pub port : u16,
    pub benchmark : bool,
    pub throttle : bool
}

pub enum EncodingQuality {
    LossyHigh,
    LossyMedium,
    LossyMediumInterframeComparison,
    LossyLow,
    Lossless
}
impl EncodingQuality {
    fn get_rfb_encodings(&self) -> Vec<i32> {
        use EncodingQuality::*;
        match *self {
            LossyHigh => vec![
                rfb::ENCODING_WORST_JPEG_QUALITY + 95,
                rfb::ENCODING_CHROMA_SUBSAMPLING_1X,
                rfb::ENCODING_COMPRESSION_LEVEL_0 + 1],
            LossyMedium => vec![
                rfb::ENCODING_WORST_JPEG_QUALITY + 80,
                rfb::ENCODING_CHROMA_SUBSAMPLING_2X,
                rfb::ENCODING_COMPRESSION_LEVEL_0 + 1],
            LossyMediumInterframeComparison => vec![
                rfb::ENCODING_WORST_JPEG_QUALITY + 80,
                rfb::ENCODING_CHROMA_SUBSAMPLING_2X,
                rfb::ENCODING_COMPRESSION_LEVEL_0 + 1 + 5],
            LossyLow => vec![
                rfb::ENCODING_WORST_JPEG_QUALITY + 30,
                rfb::ENCODING_CHROMA_SUBSAMPLING_4X,
                rfb::ENCODING_COMPRESSION_LEVEL_0 + 2 + 5],
            Lossless => vec![rfb::ENCODING_COMPRESSION_LEVEL_0 + 1 + 5]
        }
    }
}
fn get_rfb_encodings(encoding_quality : EncodingQuality) -> Vec<i32> {
    let mut encodings = vec![
            rfb::ENCODING_TIGHT,
            rfb::ENCODING_RAW,
            
            rfb::ENCODING_CURSOR,
            rfb::ENCODING_EXTENDED_DESKTOP_SIZE,

            rfb::ENCODING_LAST_RECT,
            rfb::ENCODING_CONTINUOUS_UPDATES,
            rfb::ENCODING_FENCE
    ];
    encodings.append(&mut encoding_quality.get_rfb_encodings());
    encodings
}

pub enum GuiEvent {
    Pointer {
        state : u8,
        x : i32,
        y : i32
    },
    RelativePointer {
        state : u8,
        dx : f64,
        dy : f64
    },
    Keyboard {
        key : u32,
        down : bool
    },
    SetEncodingQuality(EncodingQuality),
    Resized(FbSize)
}

#[derive(Clone, Copy)]
pub struct Hotspot(pub usize, pub usize);
#[derive(Clone, Copy)]
pub struct CursorSize(usize, usize);
impl CursorSize {
    pub fn stride(&self) -> usize {
        4 * self.0
    }
    pub fn no_of_bytes(&self) -> usize {
        self.stride() * self.1
    }
}
pub struct Cursor {
    changed : bool,
    rgba : Vec<u8>,
    size : CursorSize,
    hotspot : Hotspot
}
impl Cursor {
    pub fn new() -> Self {
        Self {
            changed: false,
            rgba: Vec::new(),
            size: CursorSize(0, 0),
            hotspot: Hotspot(0, 0)
        }
    }

    pub fn change_data(&mut self, rgba : Vec<u8>, size : CursorSize,
                       hotspot : Hotspot) {
        self.changed = true;
        self.rgba = rgba;
        self.size = size;
        self.hotspot = hotspot;
    }

    pub fn rgba(&self) -> &Vec<u8> {
        &self.rgba
    }
    pub fn hotspot(&self) -> Hotspot {
        self.hotspot
    }
    pub fn size(&self) -> CursorSize {
        self.size
    }

    pub fn handle_changed(&mut self) -> bool {
        let ret = self.changed;
        self.changed = false;
        ret
    }
}

struct CursorDifference {
    x : f64,
    y : f64
}
impl CursorDifference {
    fn new() -> Self {
        Self {
            x: 0.0,
            y: 0.0
        }
    }

    fn remove_integer_parts(&mut self) -> (i32, i32) {
        let mut x = 0;
        let mut y = 0;

        if self.x >= 1.0 || self.x <= -1.0 {
            x = self.x as i32;
            self.x -= x as f64;
        }
        if self.y >= 1.0 || self.y <= -1.0 {
            y = self.y as i32;
            self.y -= y as f64;
        }

        (x, y)
    }

    fn add(&mut self, x : f64, y : f64) {
        self.x += x;
        self.y += y;
    }
}

//fn reserve_for(buffer : &SharedBuf, size : usize) {
//    let mut buffer = buffer.borrow_mut();
//    let len = buffer.len();
//    if size > buffer.capacity() {
//        buffer.reserve(size - len);
//    }
//    unsafe {
//        buffer.set_len(size);
//    }
//}

pub enum ProtocolEvent {
    ChangeDisplaySize(FbSize),
    UpdateFramebuffer(Vec<u8>, FbSize),
    UpdateCursor(Vec<u8>, CursorSize, Hotspot),
    SetTitle(String)
}
pub trait View {
    type Output : ViewOutput;

    fn change_display_size_to(&self, size : FbSize) {
        self.get_output().handle_event(ProtocolEvent::ChangeDisplaySize(size));
    }
    fn set_title(&self, title : String) {
        self.get_output().handle_event(ProtocolEvent::SetTitle(title));
    }

    fn get_output(&self) -> &Self::Output;
    fn get_events(&mut self) -> mpsc::Receiver<GuiEvent>;
    fn desired_pixel_format() -> PixelFormat;
}
pub trait ViewOutput : Send + Clone + 'static {
    fn update_framebuffer(&self, fb_data : Vec<u8>, size : FbSize) {
        self.handle_event(ProtocolEvent::UpdateFramebuffer(fb_data, size));
    }
    fn update_framebuffer_sync(&self, fb_data : Vec<u8>, size : FbSize);
    fn update_cursor(&self, cursor : &Cursor) {
        self.handle_event(ProtocolEvent::UpdateCursor(
                cursor.rgba().clone(), cursor.size(), cursor.hotspot()));
    }

    fn handle_event(&self, event : ProtocolEvent);
}

fn in_seconds(duration : Duration) -> f64 {
    duration.as_secs() as f64 + duration.subsec_nanos() as f64 * 1e-9
}
struct Stopwatch {
    most_recent_instant : Instant
}
impl Stopwatch {
    pub fn new() -> Self {
        Self {
            most_recent_instant: Instant::now()
        }
    }

    pub fn take_measurement(&mut self, title : &str) {
        let now = Instant::now();
        let delay = in_seconds(now.duration_since(self.most_recent_instant));
        if delay >= 0.001 {
            eprintln!("stopwatch: ‘{}’ {:?} ({:?})", title, now, delay);
        }
        self.most_recent_instant = now;
    }
}
struct NullStopwatch;
impl NullStopwatch {
    pub fn new() -> Self {
        Self { }
    }
    pub fn take_measurement(&mut self, _title : &str) { }
}

struct MovingAverage {
    window_length : usize,
    samples : VecDeque<Duration>,
    mean : Duration
}
impl MovingAverage {
    pub fn new(window_length : usize) -> Self {
        Self {
            window_length: window_length,
            samples: VecDeque::new(),
            mean: Duration::from_secs(0)
        }
    }

    pub fn add(&mut self, new : Duration) {
        self.samples.push_back(new);
        if self.samples.len() > self.window_length {
            self.mean = self.mean 
                + (new / self.window_length as u32)
                - (self.samples.pop_front().unwrap()
                   / self.window_length as u32);
        } else {
            self.mean += new / self.window_length as u32;
        }
    }

    pub fn get(&self) -> Duration {
        self.mean
    }
}
struct ThrottleController {
    sleep_duration : Duration,
    last_decrease : Instant,
    delay_average : MovingAverage,
    window_length : usize,
    freeze_counter : usize
}
impl ThrottleController {
    pub fn new() -> Self {
        let window_length = 50;
        Self {
            sleep_duration: Duration::from_millis(0),
            last_decrease: Instant::now(),
            delay_average: MovingAverage::new(window_length),
            window_length: window_length,
            freeze_counter: 0
        }
    }

    pub fn register_leftover_frame_delay(&mut self, delay : Duration) {
        let threshold = Duration::from_millis(1);

        self.delay_average.add(delay);
        let delay = self.delay_average.get();

        if self.freeze_counter != 0 {
            self.freeze_counter -= 1;
            return;
        }

        if delay > threshold {
            self.sleep_duration += delay;
            self.freeze_counter = self.window_length;
            eprintln!("will sleep for {}", in_seconds(self.sleep_duration));
        }

        if Instant::now().duration_since(self.last_decrease) 
            > Duration::from_millis(500)
        {
            let minus = if self.sleep_duration > Duration::from_millis(100) {
                Duration::from_millis(5)
            } else if self.sleep_duration > Duration::from_millis(50) {
                Duration::from_millis(2)
            } else {
                Duration::from_millis(1)
            };
            if self.sleep_duration >= minus {
                self.sleep_duration -= minus;
            }
            self.last_decrease = Instant::now();
        }
    }

    pub fn sleep_duration(&self) -> Duration {
        self.sleep_duration
    }
}

pub struct MainError(pub String);
impl From<io::Error> for MainError {
    fn from(err : io::Error) -> Self {
        MainError(format!("I/O error: {:?}", err))
    }
}
impl<E> From<infrastructure::thread_pool::Error<E>> for MainError 
    where MainError : From<E>
{
    fn from(err : infrastructure::thread_pool::Error<E>) -> Self {
        use infrastructure::thread_pool::Error::*;
        match err {
            Panic => MainError(String::from("panic while decoding")),
            Value(e) => MainError::from(e)
        }
    }
}
impl<E> From<Vec<E>> for MainError 
    where MainError : From<E>
{
    fn from(mut errors : Vec<E>) -> Self {
        let ret = String::from("multiple errors: ");
        let strings : Vec<_> = errors.drain(..).map(|e| MainError::from(e).0)
            .collect();
        MainError(ret + &strings.join(",\n"))
    }
}

fn parse_packet<T, I>(buffer : &SharedBuf, input : I) 
    -> Result<T, MainError>
    where T : Packet,
          I : io::Read
{
    let ret = T::parse(buffer, input);
    match ret {
        Ok(x) => Ok(x),
        Err((err, position)) => if err.is_eof() {
            Err(MainError(String::from("Connection to server broken")))
        } else {
            Err(MainError(format!(
                        "Error at position {} when parsing packet {}: {:?}",
                        position, T::name(), err)))
        }
    }
}

fn write_packet<T, O>(packet : T, output : O)
    -> Result<(), MainError>
    where T : Packet,
          O : io::Write
{
    match packet.write(output) {
        Ok(()) => Ok(()),
        Err(err) => if err.is_io_error() {
            Err(MainError(String::from(
                "Can't write packet, connection to server broken")))
        } else {
            Err(MainError(format!("Can't write packet {}: {:?}",
                                  T::name(), err)))
        }
    }
}

pub fn socket_thread_main<V : View>(
    config : ConnectionConfig,
    view : V) -> Result<(), MainError>
{
    //TODO handle differently (re-connect)?
    let host_and_port = format!("{}:{}", config.host, config.port);
    let socket = match TcpStream::connect(&host_and_port) {
        Ok(s) => s,
        Err(err) => {
            //TODO implement From and then Display for MainError
            return Err(MainError(format!("Could not connect to {}: {}", 
                                         host_and_port, err)));
        }
    };

    //TODO benchmark BufferedReader
    handle_connection(config, socket, view)
}

pub fn handle_connection<V : View>(
    config : ConnectionConfig,
    socket : TcpStream,
    view : V) -> Result<(), MainError>
{
    RfbConnection::new(config, socket, view).handle()
}

enum RfbWriteEvent {
    GuiEvent(GuiEvent),
    SetScreenLayout(Vec<rfb::Screen>),
    AllowSetDesktopSize,
    UpdateRequest { 
        incremental : bool,
        size : FbSize
    },
    EnableContinuousUpdates {
        on : bool,
        x : usize,
        y : usize,
        size : FbSize
    },
    Fence {
        flags : u32,
        payload : Vec<u8>
    },
    Heartbeat
}
struct RfbWriteEnd {
    socket : BufWriter<TcpStream>,
    input : mpsc::Receiver<RfbWriteEvent>,
    mouse_server : UdpSocket
}
impl RfbWriteEnd {
    fn write_packet<T>(&mut self, packet : T) -> Result<(), MainError>
        where T : Packet
    {
        write_packet(packet, &mut self.socket)
    }

    fn handle(&mut self) -> Result<(), MainError> {
        use GuiEvent as Gui;
        use RfbWriteEvent::*;

        let mut virtual_cursor_difference = CursorDifference::new();
        let mut previous_mouse_state = 0;
        let mut screen_layout : Vec<rfb::Screen> = Vec::new();
        let mut set_desktop_size_allowed = false;

        while let Ok(event) = self.input.recv() {
            match event {
                GuiEvent(Gui::Pointer { state, x, y }) => {
                    //eprintln!("button state: {:x}, x: {}, y: {}", state, x, y);
                    self.write_packet(rfb::ClientToServer::PointerEvent(
                            rfb::PointerEvent {
                                mask: state,
                                x: x as u16, //TODO clamp
                                y: y as u16
                            }))?;
                },
                GuiEvent(Gui::RelativePointer { state, dx, dy }) => {
                    virtual_cursor_difference.add(dx, dy);
                    let (int_dx, int_dy) = virtual_cursor_difference
                        .remove_integer_parts();
                    let state_changed = previous_mouse_state != state;
                    previous_mouse_state = state;
                    if int_dx != 0 || int_dy != 0 || state_changed {
                        let mut message = Vec::new();
//                        if state != 0 {
//                            eprintln!("{:?} sending relative mouse message {} {}", 
//                                    Instant::now(), int_dx, int_dy);
//                        }
                        protocol::VirtualMouseServerMessage {
                            button_mask: state,
                            dx: int_dx as i16,
                            dy: int_dy as i16
                        }.write(&mut message).unwrap();
                        self.mouse_server.send(&message[..])?;
                    }
                },
                GuiEvent(Gui::Keyboard { key, down }) => {
                    self.write_packet(rfb::ClientToServer::KeyEvent(
                            rfb::KeyEvent {
                                down: down,
                                key: key
                            }))?;
                },
                GuiEvent(Gui::Resized(new_size)) => {
//                        if new_size != self.framebuffer.size()
                    if set_desktop_size_allowed {
                        let screens = screen_layout.clone();
                        self.write_packet(
                            rfb::ClientToServer::SetDesktopSize(
                                rfb::SetDesktopSize {
                                    width: new_size.width,
                                    height: new_size.height,
                                    screens: screens
                                }))?;
                    }
                },
                GuiEvent(Gui::SetEncodingQuality(new_quality)) => {
                    self.send_set_encodings(new_quality)?;
                },
                SetScreenLayout(layout) => {
                    screen_layout = layout
                },
                AllowSetDesktopSize => {
                    set_desktop_size_allowed = true;
                },
                UpdateRequest { incremental, size } => {
                    let request = rfb::FramebufferUpdateRequest {
                        incremental: incremental,
                        x: 0,
                        y: 0,
                        width: size.width,
                        height: size.height
                    };
                    self.write_packet(rfb::ClientToServer
                                      ::FramebufferUpdateRequest(request))?;
                },
                EnableContinuousUpdates { on, x, y, size } => {
                    let message = rfb::EnableContinuousUpdates {
                        enable: on,
                        x: x,
                        y: y,
                        width: size.width,
                        height: size.height
                    };
                    self.write_packet(rfb::ClientToServer
                                      ::EnableContinuousUpdates(message))?;

                },
                Fence { flags, payload } => {
                    let message = rfb::Fence {
                        flags: flags,
                        payload: payload
                    };
                    self.write_packet(rfb::ClientToServer
                                      ::Fence(message))?;
                },
                Heartbeat => { }
            }
            self.socket.flush()?;
        }
        Ok(())
    }

    fn send_set_encodings(&mut self, encoding_quality : EncodingQuality)
        ->  Result<(), MainError>
    {
        self.write_packet(rfb::ClientToServer::SetEncodings(rfb::SetEncodings {
            encodings: get_rfb_encodings(encoding_quality)
        }))
    }

}

fn disable_nagles_algo(socket : &TcpStream) -> io::Result<()> {
    use std::os::unix::io::AsRawFd;

    let one : libc::c_int = 1;
    let socket_fd = socket.as_raw_fd();
    let ret;
    unsafe {
        ret = libc::setsockopt(
            socket_fd, libc::SOL_TCP, libc::TCP_NODELAY, 
            &one as *const libc::c_int as *const libc::c_void, 
            std::mem::size_of::<libc::c_int>() as libc::socklen_t);
    }
    if ret != 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

struct RfbConnection<V : View> {
    config : ConnectionConfig,
    socket : BufReader<TcpStream>,
    view : V,
    buffer : SharedBuf,
    framebuffer : SharedFb,
    write_end_sender : Option<mpsc::Sender<RfbWriteEvent>>
}
impl<V : View> RfbConnection<V> {
    fn new(config : ConnectionConfig, socket : TcpStream, view : V) 
        -> Self
    {
        let buffer = RefCell::new(Vec::new());
        Self {
            config: config,
            socket: BufReader::new(socket),
            view: view,
            buffer: buffer,
            framebuffer: Arc::new(ModeLock::new(Framebuffer::new())),
            write_end_sender: None
        }
    }

    fn parse_packet<T>(&mut self) -> Result<T, MainError>
        where T : Packet
    {
        parse_packet(&self.buffer, &mut self.socket)
    }
    fn write_packet<T>(&mut self, packet : T) -> Result<(), MainError>
        where T : Packet
    {
        write_packet(packet, self.socket.get_mut())
    }

    fn read_bytes(&mut self, length : usize) -> io::Result<Vec<u8>> {
        let mut bytes = Vec::with_capacity(length);
        unsafe {
            bytes.set_len(length);
        }
        self.socket.read_exact(&mut bytes[..])?;
        Ok(bytes)
    }
    fn read_zlib_data(&mut self, stream_no : usize, uncompressed_size : usize)
        -> Result<TightData, MainError>
    {
        Ok(if uncompressed_size < 12 {
            TightData::UncompressedRgb(self.read_bytes(uncompressed_size)?)
        } else {
            let size = self.parse_packet::<rfb::TightZlib>()?.length;
            TightData::CompressedRgb {
                stream_no: stream_no,
                bytes: self.read_bytes(size)?
            }
        })
    }

    fn resize_fb(&mut self, new_size : FbSize) {
        self.view.change_display_size_to(new_size);
        self.framebuffer.lock(FbAccess::Resizing).resize(new_size);
        self.write_end().send(RfbWriteEvent::EnableContinuousUpdates {
            on: true,
            x: 0,
            y: 0,
            size: new_size
        }).unwrap_or(());
    }

    fn fb_size(&self) -> FbSize {
        self.framebuffer.lock(FbAccess::Reading).size()
    }

    fn write_end(&self) -> &mpsc::Sender<RfbWriteEvent> {
        self.write_end_sender.as_ref().unwrap()
    }
    fn send_fb_update_request(&mut self, incremental : bool, size : FbSize) {
        self.write_end().send(RfbWriteEvent::UpdateRequest {
            incremental: incremental,
            size: size
        }).unwrap_or(());
    }

    fn handle(&mut self) -> Result<(), MainError>  {
        let server_init = self.setup()?;
        
        self.view.set_title(server_init.name.clone());
        self.write_packet(rfb::ClientToServer::SetEncodings(rfb::SetEncodings {
            encodings: get_rfb_encodings(EncodingQuality::LossyHigh)
        }))?;
        self.write_packet(rfb::ClientToServer::SetPixelFormat(
                rfb::SetPixelFormat {
                    format: PIXEL_FORMAT.clone()
                }
        ))?;

        disable_nagles_algo(self.socket.get_ref())?;

        self.handle_main_part(server_init)
        //TODO call exit on view instead
    //    glib::idle_add(|| {
    //        gtk::main_quit();
    //        glib::Continue(false)
    //    });

    //    Ok(())
    }

    fn setup(&mut self) -> Result<rfb::ServerInit, MainError> {
        let _server_address = self.socket.get_ref().peer_addr().unwrap();

        let protocol_version = self.parse_packet::<rfb::ProtocolVersion>()?;
        self.write_packet(protocol_version)?;

        let security_types = 
            match self.parse_packet::<rfb::SecurityTypes>()? {
                rfb::SecurityTypes::ErrorReason(error_message) => 
                    return Err(MainError(format!(
                            "Server error after version negotiation:\n{}", 
                            error_message.string))),
                rfb::SecurityTypes::SecurityTypesArray(x) => x
            };
        if !security_types.types.iter().any(|&x| x == rfb::SEC_TYPE_NONE) {
            return Err(MainError(String::from(
                    "Server requires authentication. Not implemented yet.")))
        }
        
        //eprintln!("received[{}] ‘{:?}’", server_address, security_types.types);
        self.write_packet(rfb::SecurityResponse {
            sec_type: rfb::SEC_TYPE_NONE
        })?;

        let security_result = self.parse_packet::<rfb::SecurityResult>()?;
        if let rfb::SecurityResult::Failed(reason) = security_result {
            return Err(MainError(
                    format!("Failed security handshake: {}", reason.string)));
        }

        self.write_packet(rfb::ClientInit {
            shared: true
        })?;

        self.parse_packet::<rfb::ServerInit>()
    }

    fn handle_main_part(&mut self, server_init : rfb::ServerInit)
        -> Result<(), MainError>
    {
        //TODO non-blocking select between GUI and server? (use try_recv for mpsc)
        
        let view_output_clone = self.view.get_output().clone();
        let fb_updater = ThreadPool::new("fb-updater", 1, move || {
            view_output_clone.clone()
        });
        let mut last_fb_update : Option<Future<MainError>> = None;

        let cursor = Arc::new(Mutex::new(Cursor::new()));
        let decoder = Arc::new(Mutex::new(DecodingMaster::new(
            self.framebuffer.clone(),
            cursor.clone())));

        let (write_end_sender, write_end_receiver) = mpsc::channel();
        let write_end_sender_clone = write_end_sender.clone();
        self.write_end_sender = Some(write_end_sender);
        self.resize_fb(FbSize::new(server_init.width, server_init.height));

        let mouse_server = UdpSocket::bind("0.0.0.0:0")?;
        mouse_server.connect(
            (&self.config.host[..], self.config.port - 5900 + 5100))?;
        let write_end_socket = self.socket.get_ref().try_clone().unwrap();
        let write_end = std::thread::spawn(move || {
            RfbWriteEnd {
                socket: BufWriter::new(write_end_socket),
                input: write_end_receiver,
                mouse_server: mouse_server
            }.handle()
        });
        let gui_events = self.view.get_events();
        std::thread::spawn(move || {
            while let Ok(event) = gui_events.recv() {
                write_end_sender_clone.send(RfbWriteEvent::GuiEvent(event))
                    .unwrap_or(());
            }
        });

        let fb_size = self.fb_size();
        self.send_fb_update_request(false, fb_size);
        let _start = Instant::now();

        let mut maybe_new_fb_size = None;
        let mut area = 0.0;
        let mut no_of_successive_full_updates = 0;
        let mut zero_copy_mode = false;

        let mut throttle_controller = ThrottleController::new();

        let mut in_1_second = Instant::now() + Duration::from_secs(1);
        let mut fps : f64 = 0.0;
        let mut time_spent_waiting = Duration::from_secs(0);
//        let mut stopwatch = Stopwatch::new();
        let mut stopwatch = NullStopwatch::new();
        loop {
            stopwatch.take_measurement("start");
            if let Err(_) = self.write_end().send(RfbWriteEvent::Heartbeat) {
                return Err(write_end.join().unwrap().unwrap_err());
            }

            stopwatch.take_measurement("waiting for packet");
            let before_waiting_for_fb_update = Instant::now();
            let server_packet = self.parse_packet::<rfb::ServerToClient>()?;
            time_spent_waiting += Instant::now().duration_since(
                before_waiting_for_fb_update);
//            let message_type = match server_packet {
//                rfb::ServerToClient::FramebufferUpdate(_) => "update",
//                rfb::ServerToClient::Fence(_) => "fence",
//                _ => "other"
//            };
//            eprintln!("waited: {} for {} ({})", 
//                      in_seconds(Instant::now().duration_since(
//                              before_waiting_for_fb_update)),
//                      message_type,
//                      in_seconds(Instant::now().duration_since(_start)));
            stopwatch.take_measurement("got packet");
            match server_packet {
                rfb::ServerToClient::FramebufferUpdate(update) => {
                    let start = Instant::now();
                    if last_fb_update.is_some() {
                        last_fb_update.take().unwrap().wait()?;
                    }
                    if self.config.throttle {
                        throttle_controller.register_leftover_frame_delay(
                            Instant::now().duration_since(start));
                    }

                    if self.config.benchmark {
                        let fb_area = self.fb_size().width as f64
                            * self.fb_size().height as f64;
                        fps += area / fb_area;
                        area = 0.0;
                        if Instant::now() >= in_1_second {
                            let fps_without_waiting = 
                                fps / (1.0 - in_seconds(time_spent_waiting));
                            let fps_without_waiting = fps_without_waiting.round();
                            println!("{} {}", fps.round(), 
                                     fps_without_waiting);
                            ::std::io::Write::flush(
                                &mut ::std::io::stdout()).unwrap();
                            fps = 0.0;
                            time_spent_waiting = Duration::from_secs(0);
                            in_1_second += Duration::from_secs(1);
                        }
                    }

                    if let Some(new_fb_size) = maybe_new_fb_size {
                        self.resize_fb(new_fb_size);
                        maybe_new_fb_size = None;
                    }

                    //TODO not required if continuous updates are enabled
//                    self.send_fb_update_request(true, self.fb_size());

                    stopwatch.take_measurement(
                        "before reading update (after finalizing decoding)");

//                    eprintln!("received[{}] ‘{:?}’", server_address, update);
                    let fb_size = self.fb_size();
                    let (decoding, area_this_update) = 
                        self.read_rectangles_and_start_decoding(
                            update,
                            &*decoder.lock().unwrap(),
                            &mut maybe_new_fb_size,
                            &mut time_spent_waiting)?;

                    area += area_this_update as f64;
                    if area_this_update == fb_size.no_of_pixels() {
                        no_of_successive_full_updates += 1;
                        if no_of_successive_full_updates == 60 {
                            zero_copy_mode = true;
                            eprintln!("zero-copy mode on");
                        }
                    } else {
                        if zero_copy_mode {
                            eprintln!("zero-copy mode off");
                        }
                        self.send_fb_update_request(false, fb_size);
                        no_of_successive_full_updates = 0;
                        zero_copy_mode = false;
                    }

                    let decoder = decoder.clone();
                    let cursor = cursor.clone();
                    let fb = self.framebuffer.clone();
                    let throttle = self.config.throttle;
                    last_fb_update = Some(fb_updater.spawn_fn(move |view| {
//                        let mut stopwatch = Stopwatch::new();
//                        stopwatch.take_measurement("before decoding");
                        decoding.wait()?;
//                        stopwatch.take_measurement("after decoding");

                        let desired_pixel_format = V::desired_pixel_format();
                        let (fb, size) = match desired_pixel_format {
                            PixelFormat::NativeBgrx if zero_copy_mode => {
                                let mut fb = fb.lock(FbAccess::Decoding);
                                let size = fb.size();
                                let mut new_fb = unsafe {
                                    Framebuffer::uninitialized(size)
                                };
                                std::mem::swap(&mut new_fb, &mut fb);
                                (new_fb.take_data(), size)
                            },
                            _ => {
                                decoder.lock().unwrap()
                                    .convert_or_copy_fb(desired_pixel_format)
                            }
                        };

                        let mut cursor = cursor.lock().unwrap();
                        if cursor.handle_changed() {
                            view.update_cursor(&cursor)
                        }

                        if !throttle {
                            view.update_framebuffer(fb, size);
                        } else {
                            view.update_framebuffer_sync(fb, size);
                        }

                        Ok(())
                    }));

                    if self.config.throttle {
                        std::thread::sleep(
                            throttle_controller.sleep_duration());
                    }

                    stopwatch.take_measurement("after reading update");
                },
                rfb::ServerToClient::Fence(fence) => { 
//                    eprintln!("fence: {:?}", fence);
                    let mut flags = fence.flags;
                    if fence.flags & rfb::FENCE_REQUEST != 0 {
                        flags &= !rfb::FENCE_REQUEST;
                        flags &= rfb::FENCE_BLOCK_BEFORE 
                            | rfb::FENCE_BLOCK_AFTER;
                        self.write_end().send(RfbWriteEvent::Fence {
                            flags: flags,
                            payload: fence.payload
                        }).unwrap_or(());
                    }
                },
                rfb::ServerToClient::EndOfContinuousUpdates(_) => { },
                rfb::ServerToClient::ServerCutText(_) => { },
                rfb::ServerToClient::Bell(_) => { }
            }
        }
    }

    fn read_rectangles_and_start_decoding(
        &mut self, update : rfb::FramebufferUpdate, 
        decoder : &DecodingMaster, maybe_new_fb_size : &mut Option<FbSize>,
        time_spent_waiting : &mut Duration)
        -> Result<(FutureCollection<MainError>, usize), MainError>
    {
        let mut area : usize = 0;
        for _ in 0..update.no_of_rectangles {
            //TODO bounds check
            let rectangle = self.parse_packet::<rfb::Rectangle>()?;
            area += rectangle.width * rectangle.height;
//      eprintln!("received[{}] ‘{:?}’", server_address, rectangle);
            let (last_rect, new_fb_size) = 
                self.handle_rectangle(
                    decoder, 
                    rectangle, 
                    time_spent_waiting)?;
            if let Some(new_fb_size) = new_fb_size {
                *maybe_new_fb_size = Some(new_fb_size);
            }
            if last_rect {
                break;
            }
        }
        Ok((decoder.finish(), area))
    }

    fn handle_rectangle(&mut self,
                        decoder : &DecodingMaster,
                        rectangle : rfb::Rectangle,
                        time_spent_waiting : &mut Duration)
        -> Result<(bool, Option<FbSize>), MainError>
    {
        match rectangle.payload {
            //TODO refactor to e.g. rfb::rect::Raw with a pub type
            rfb::RectanglePayload::RawRectangle(_) => {
                let size = ((PIXEL_FORMAT
                            .bits_per_pixel as usize) / 8)
                    * rectangle.width
                    * rectangle.height;
            
                let start = Instant::now();
                let bytes = self.read_bytes(size)?;
                *time_spent_waiting += Instant::now().duration_since(start);

                decoder.accept(DecodingJob::rect_from_rfb(
                        &rectangle,
                        EncodingMethod::RawBgra(bytes)));
            },
            rfb::RectanglePayload::TightRectangle(ref payload) => {
                let zlib_reset_map = payload.control_byte & 0x0f;
                if zlib_reset_map & 0x01 != 0 {
                    decoder.accept(DecodingJob::ResetZlib(0));
                }
                if zlib_reset_map & 0x02 != 0 {
                    decoder.accept(DecodingJob::ResetZlib(1));
                }
                if zlib_reset_map & 0x04 != 0 {
                    decoder.accept(DecodingJob::ResetZlib(2));
                }
                if zlib_reset_map & 0x08 != 0 {
                    decoder.accept(DecodingJob::ResetZlib(3));
                }
                let zlib_stream_no = (payload.control_byte & 0x30) >> 4;
                let zlib_stream_no = zlib_stream_no as usize;

                match payload.method {
                    rfb::TightMethod::Fill(_) => {
                        let color = self.parse_packet::<rfb::TPixel>()?;
                        decoder.accept(DecodingJob::rect_from_rfb(
                                &rectangle,
                                EncodingMethod::Fill(
                                    Bgrx::from_tpixel(color))
                                ));
                    },
                    rfb::TightMethod::Basic(ref basic) => {
                        match basic.filter {
                            rfb::TightFilter::PaletteFilter(ref palette) => {
                                let mut colors = Vec::with_capacity(palette.no_of_colors);
                                for _ in 0..palette.no_of_colors {
                                    colors.push(Bgrx::from_tpixel(self.parse_packet::<rfb::TPixel>()?));
                                }

                                let stride = if palette.no_of_colors == 2 {
                                    (rectangle.width + 7) / 8
                                } else {
                                    rectangle.width
                                };
                                let uncompressed_size = rectangle.height * stride;

                                let data = self.read_zlib_data(
                                    zlib_stream_no,
                                    uncompressed_size)?;
                                decoder.accept(DecodingJob::rect_from_rfb(
                                        &rectangle,
                                        EncodingMethod::PaletteFilter(colors, data)));
                                //TODO error out if no_of_colors is 1 (in syntax?)
                            },
                            _ => {
                                unimplemented!()
                            }
                        }
                    },
                    rfb::TightMethod::BasicNoFilterId(_) => {
                        //TODO do the same as here for explicit copy filter
                        let uncompressed_size = rectangle.width
                            * rectangle.height * TPIXEL_SIZE;
                        let data = self.read_zlib_data(
                            zlib_stream_no,
                            uncompressed_size)?;
                        decoder.accept(DecodingJob::rect_from_rfb(
                                &rectangle,
                                EncodingMethod::CopyFilter(data)));
                    },
                    rfb::TightMethod::Jpeg(ref jpeg) => {
                        let bytes = self.read_bytes(jpeg.length)?;
                        decoder.accept(DecodingJob::rect_from_rfb(
                                &rectangle,
                                EncodingMethod::Jpeg(bytes)));
                    }
                }
            },
            rfb::RectanglePayload::CursorRectangle(_) => {
                //TODO parse rectangle.size
                let no_of_pixel_bytes = rectangle.width * rectangle.height
                    * PIXEL_FORMAT_BYTES_PER_PIXEL;
                let bitmask_stride = (rectangle.width + 7) / 8;
                decoder.accept(DecodingJob::rect_from_rfb(
                        &rectangle,
                        EncodingMethod::CursorBgrx {
                            pixels: self.read_bytes(no_of_pixel_bytes)?,
                            bitmask: self.read_bytes(
                                bitmask_stride * rectangle.height)?
                        }));
            },
            rfb::RectanglePayload::DesktopSizeRectangle(_) => {
//                                eprintln!("received[{}] ‘{:?}’", server_address, rectangle);
                return Ok((false, Some(FbSize::new(rectangle.width, 
                                                   rectangle.height))));
            },
            rfb::RectanglePayload::ExtendedDesktopSizeRectangle(rect) => {
//                                eprintln!("received[{}] ‘{:?}’", server_address, rectangle);
                self.write_end().send(RfbWriteEvent::AllowSetDesktopSize).unwrap_or(());
                if rectangle.y == rfb::EXTENDED_DESKTOP_NO_ERROR {
                    self.write_end().send(RfbWriteEvent::SetScreenLayout(rect.screens)).unwrap_or(());
                    return Ok((false, Some(FbSize::new(rectangle.width,
                                                       rectangle.height))));
                }
            },
            rfb::RectanglePayload::LastRectangle(_) => {
                return Ok((true, None));
            }
        }
        Ok((false, None))
    }
}

pub fn run(args : Vec<String>) {
    //TODO pass &str
    //TODO handle parse errors
    let host_and_port : Vec<&str> = args[1].split(":").collect();
    let options : Vec<_> = args.iter()
        .skip(2)
        .take_while(|&s| s.starts_with("--"))
        .map(|s| s.as_str())
        .collect();

    let config = ConnectionConfig {
        host: String::from(host_and_port[0]),
        port: u16::from_str(host_and_port[1]).unwrap(),
        benchmark: options.contains(&"--benchmark"),
        throttle: options.contains(&"--throttle")
    };

    if options.contains(&"--sdl") {
        sdl_frontend::run(config);
    } else {
        gtk_frontend::run(config);
    }
}

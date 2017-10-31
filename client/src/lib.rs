#![feature(conservative_impl_trait)]
#![feature(const_fn)]
#![feature(drop_types_in_const)]
#![feature(test)]

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

use std::io;
use std::io::{BufReader,BufWriter,Read,Write};
use std::ptr;
//use std::io::{Read,Write};
use std::net::{TcpStream,UdpSocket};
use std::sync::mpsc;

mod presentation;
use presentation::gtk as gtk_frontend;

pub mod protocol;
mod encoding;
use encoding::{DecodingJob,DecodingMaster,EncodingMethod,TightData};
mod tight;

use protocol::rfb;
use protocol::parsing::io_input::SharedBuf;
use protocol::parsing::Packet;
use std::cell::RefCell;
use std::sync::{Arc,RwLock};
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

struct ViewPixelFormat {
    bytes_per_pixel : usize
}
const VIEW_PIXEL_FORMAT : ViewPixelFormat = ViewPixelFormat {
    bytes_per_pixel: 3
};

//const CURSOR_BYTES_PER_PIXEL : usize = 4;

pub struct ConnectionConfig {
    pub host : String,
    pub port : u16,
    pub benchmark : bool
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
            
            //rfb::ENCODING_CURSOR,
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

//TODO move to different file
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FbSize {
    width: usize, 
    height: usize
}
impl FbSize {
    pub fn new(width : usize, height : usize) -> Self {
        Self {
            width: width,
            height: height
        }
    }
    pub fn no_of_pixels(&self) -> usize {
        self.width * self.height
    }
}
type SharedFb = Arc<RwLock<Framebuffer>>;
pub enum ProtocolEvent {
    ChangeDisplaySize(FbSize),
    UpdateFramebuffer(SharedFb),
    SetTitle(String)
}
pub trait View {
    fn change_display_size_to(&self, size : FbSize) {
        self.handle_event(ProtocolEvent::ChangeDisplaySize(size));
    }
    fn update_framebuffer(&self, new_fb : SharedFb) {
        self.handle_event(ProtocolEvent::UpdateFramebuffer(new_fb));
    }
    fn set_title(&self, title : String) {
        self.handle_event(ProtocolEvent::SetTitle(title));
    }

    fn handle_event(&self, event : ProtocolEvent);
    fn get_events(&mut self) -> mpsc::Receiver<GuiEvent>;
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
        eprintln!("stopwatch: ‘{}’ {:?} ({:?})", title, now, 
                  in_seconds(now.duration_since(self.most_recent_instant)));
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

pub struct MainError(pub String);
impl From<io::Error> for MainError {
    fn from(err : io::Error) -> Self {
        MainError(format!("I/O error: {:?}", err))
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

#[repr(C)]
pub struct Rgb {
    pub r : u8,
    pub g : u8,
    pub b : u8
}
impl Rgb {
    pub fn from_tpixel(tpixel : rfb::TPixel) -> Self {
        Self {
            r: tpixel.r,
            g: tpixel.g,
            b: tpixel.b,
        }
    }
}

pub struct Framebuffer {
    data : Vec<u8>,
    size : FbSize
}
impl Framebuffer {
    fn new() -> Self {
        Self {
            data: Vec::new(),
            size: FbSize::new(0, 0)
        }
    }

    fn resize(&mut self, new_size : FbSize) {
        //TODO pixel data is not correctly transferred in this way (you must crop right-most columns and
        //bottom-most rows)
        self.size = new_size;
        let new_len = VIEW_PIXEL_FORMAT.bytes_per_pixel 
            * new_size.no_of_pixels();
        let gray = 0xe0u8;
        self.data.resize(new_len, gray);
    }

    pub fn size(&self) -> FbSize {
        self.size
    }

    pub fn data(&self) -> &Vec<u8> {
        &self.data
    }

    fn stride(&self) -> usize {
        VIEW_PIXEL_FORMAT.bytes_per_pixel * self.size.width
    }
    fn byte_pos(&self, x : usize, y : usize) -> usize {
        y * self.stride() + x * VIEW_PIXEL_FORMAT.bytes_per_pixel
    }

    fn set_pixel(&mut self, x : usize, y : usize,
                 r : u8, g : u8, b : u8) {
        let pos = self.byte_pos(x, y);
        self.data[pos] = r;
        self.data[pos + 1] = g;
        self.data[pos + 2] = b;
    }

    fn set_line(&mut self, x : usize, width : usize, y : usize, line : &[Rgb]) {
        assert!(x + width <= self.size.width);
        assert!(y <= self.size.height);
        let pos = self.byte_pos(x, y);
        unsafe {
            let data = self.data.as_mut_ptr().offset(pos as isize) as *mut Rgb;
            ptr::copy_nonoverlapping(line.as_ptr(), data, width);
        }
    }
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
//                        eprintln!("button state: {:x}, x: {}, y: {}", state, x, y);
                    self.write_packet(rfb::ClientToServer::PointerEvent(
                            rfb::PointerEvent {
                                mask: state,
                                x: x as u16, //TODO clamp
                                y: y as u16
                            }))?;
                },
                GuiEvent(Gui::RelativePointer { state, dx, dy }) => {
                    virtual_cursor_difference.add(dx, dy);
                    let (int_dx, int_dy) = virtual_cursor_difference.remove_integer_parts();
                    let state_changed = previous_mouse_state != state;
                    previous_mouse_state = state;
                    if int_dx != 0 || int_dy != 0 || state_changed {
                        let mut message = Vec::new();
                        //eprintln!("{:?} sending relative mouse message", Instant::now());
                        protocol::VirtualMouseServerMessage {
                            button_mask: state,
                            dx: int_dx as i8,
                            dy: int_dy as i8
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
    use std::os::unix::io::IntoRawFd;

    let one : libc::c_int = 1;
    let socket_fd = socket.try_clone().unwrap().into_raw_fd();
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
            framebuffer: Arc::new(RwLock::new(Framebuffer::new())),
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
        self.framebuffer.write().unwrap().resize(new_size);
        self.write_end().send(RfbWriteEvent::EnableContinuousUpdates {
            on: true,
            x: 0,
            y: 0,
            size: new_size
        }).unwrap_or(());
    }

    fn fb_size(&self) -> FbSize {
        self.framebuffer.read().unwrap().size()
    }

    fn write_end(&self) -> &mpsc::Sender<RfbWriteEvent> {
        self.write_end_sender.as_ref().unwrap()
    }
    fn send_fb_update_request(&mut self, incremental : bool) {
        self.write_end().send(RfbWriteEvent::UpdateRequest {
            incremental: incremental,
            size: self.fb_size()
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
        //TODO first: decode everything here, then: use worker threads and *benchmark this*!
        
        let mut decoder = DecodingMaster::new(self.framebuffer.clone());

        let mut jpeg_duration = Duration::from_secs(0);
        let mut jpeg_times = 0;

        let (write_end_sender, write_end_receiver) = mpsc::channel();
        let write_end_sender_clone = write_end_sender.clone();
        let write_end_sender_clone_2 = write_end_sender.clone();
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
        //causes the server to respond more often
//        std::thread::spawn(move || {
//            loop {
//                std::thread::sleep(Duration::from_millis(2));
//                write_end_sender_clone_2.send(RfbWriteEvent::GuiEvent(
//                        GuiEvent::Keyboard {
//                            key: 0x20,
//                            down: false
//                        })).unwrap_or(());
//            }
//        });

        self.send_fb_update_request(false);
//        self.write_end().send(RfbWriteEvent::Fence {
//            flags: rfb::FENCE_REQUEST | rfb::FENCE_SYNC_NEXT,
//            payload: Vec::new()
//        }).unwrap_or(());
        let start = Instant::now();

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
            //TODO do not block when waiting for the FramebufferUpdate

            stopwatch.take_measurement("waiting for update");
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
//                      in_seconds(Instant::now().duration_since(start)));
            match server_packet {
                rfb::ServerToClient::FramebufferUpdate(update) => {
                    stopwatch.take_measurement("update");
                    let mut area = 0.0;
                    //TODO not required if continuous updates are enabled
//                    self.send_fb_update_request(true);

//                    eprintln!("received[{}] ‘{:?}’", server_address, update);
                    let _before_decoding = Instant::now();
                    for _ in 0..update.no_of_rectangles {
                        //TODO bounds check
                        let rectangle = self.parse_packet::<rfb::Rectangle>()?;
                        area += rectangle.width as f64 
                            * rectangle.height as f64;
//                        eprintln!("received[{}] ‘{:?}’", server_address, rectangle);
                        if self.handle_rectangle(&mut decoder, rectangle,
                                                 &mut time_spent_waiting,
                                                 &mut jpeg_duration,
                                                 &mut jpeg_times)? {
                            break;
                        }
                    }
//                    eprintln!("decoding took {}", in_seconds(
//                            Instant::now().duration_since(_before_decoding)));

                    if self.config.benchmark {
                        let fb_area = self.fb_size().width as f64
                            * self.fb_size().height as f64;
                        fps += area / fb_area;
                        if Instant::now() >= in_1_second {
                            let fps_without_waiting = 
                                fps / (1.0 - in_seconds(time_spent_waiting));
                            let fps_without_waiting = fps_without_waiting.round();
                            println!("{} {} {}", fps.round(), fps_without_waiting,
                                     jpeg_times as f64 / in_seconds(jpeg_duration));
                            ::std::io::Write::flush(
                                &mut ::std::io::stdout()).unwrap();
                            fps = 0.0;
                            time_spent_waiting = Duration::from_secs(0);
                            in_1_second += Duration::from_secs(1);

                            jpeg_duration = Duration::from_secs(0);
                            jpeg_times = 0;
                        }
                    }

                    stopwatch.take_measurement("updating framebuffer");
                    let fb_reference = &self.framebuffer;
                    self.view.update_framebuffer(fb_reference.clone());
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

    fn handle_rectangle(&mut self,
                        decoder : &mut DecodingMaster,
                        rectangle : rfb::Rectangle,
                        time_spent_waiting : &mut Duration,
                        jpeg_duration : &mut Duration,
                        jpeg_times : &mut u32)
        -> Result<bool, MainError>
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
                        EncodingMethod::RawBgra(bytes)))?;
            },
            rfb::RectanglePayload::TightRectangle(ref payload) => {
                let zlib_reset_map = payload.control_byte & 0x0f;
                if zlib_reset_map & 0x01 != 0 {
                    decoder.accept(DecodingJob::ResetZlib(0))?;
                }
                if zlib_reset_map & 0x02 != 0 {
                    decoder.accept(DecodingJob::ResetZlib(1))?;
                }
                if zlib_reset_map & 0x04 != 0 {
                    decoder.accept(DecodingJob::ResetZlib(2))?;
                }
                if zlib_reset_map & 0x08 != 0 {
                    decoder.accept(DecodingJob::ResetZlib(3))?;
                }
                let zlib_stream_no = (payload.control_byte & 0x30) >> 4;
                let zlib_stream_no = zlib_stream_no as usize;

                match payload.method {
                    rfb::TightMethod::Fill(_) => {
                        let color = self.parse_packet::<rfb::TPixel>()?;
                        let mut framebuffer = self.framebuffer.write().unwrap();
                        for y in 0..rectangle.height {
                            for x in 0..rectangle.width {
                                framebuffer.set_pixel(
                                    x + rectangle.x,
                                    y + rectangle.y,
                                    color.r,
                                    color.g,
                                    color.b);
                            }
                        }
                    },
                    rfb::TightMethod::Basic(ref basic) => {
                        match basic.filter {
                            rfb::TightFilter::PaletteFilter(ref palette) => {
                                let mut colors = Vec::with_capacity(palette.no_of_colors);
                                for _ in 0..palette.no_of_colors {
                                    colors.push(Rgb::from_tpixel(self.parse_packet::<rfb::TPixel>()?));
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
                                        EncodingMethod::PaletteFilter(colors, data)))?;
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
                                EncodingMethod::CopyFilter(data)))?;
                    },
                    rfb::TightMethod::Jpeg(ref jpeg) => {
                        let start = Instant::now();
                        let bytes = self.read_bytes(jpeg.length)?;
                        decoder.accept(DecodingJob::rect_from_rfb(
                                &rectangle,
                                EncodingMethod::Jpeg(bytes)))?;

                        *jpeg_duration += Instant::now().duration_since(start);
                        *jpeg_times += 1;
                    }
                }
            },
            rfb::RectanglePayload::CursorRectangle(_rect) => {
                //TODO parse rectangle.size
                //TODO refactor out a BitmapParser from PaletteFilter + colors = 2
//                                let cursor_size = FbSize::new(rectangle.width, 
//                                                              rectangle.height);
//                                let mut cursor = Framebuffer::new();
//                                cursor.resize(cursor_size);
//
//                                let no_of_pixel_bytes = cursor_size.no_of_pixels()
//                                    * PIXEL_FORMAT_BYTES_PER_PIXEL;
//                                let src_pixels = Vec::with_capacity(no_of_pixel_bytes);
//                                unsafe {
//                                    src_pixels.set_len(no_of_pixels);
//                                }
//                                ::std::io::Read::read_exact(&mut self.socket, &mut src_pixels[..])?;
//
//                                let pixels = Vec::with_capacity(
//                                    cursor_size.no_of_pixels() * CURSOR_BYTES_PER_PIXEL);
//

//                                unimplemented!()
            },
            rfb::RectanglePayload::DesktopSizeRectangle(_) => {
//                                eprintln!("received[{}] ‘{:?}’", server_address, rectangle);
                self.resize_fb(FbSize::new(
                        rectangle.width, rectangle.height));
            },
            rfb::RectanglePayload::ExtendedDesktopSizeRectangle(rect) => {
//                                eprintln!("received[{}] ‘{:?}’", server_address, rectangle);
                self.write_end().send(RfbWriteEvent::AllowSetDesktopSize).unwrap_or(());
                if rectangle.y == rfb::EXTENDED_DESKTOP_NO_ERROR {
                    self.write_end().send(RfbWriteEvent::SetScreenLayout(rect.screens)).unwrap_or(());
                    self.resize_fb(FbSize::new(rectangle.width, rectangle.height));
                }
            },
            rfb::RectanglePayload::LastRectangle(_) => {
                return Ok(true);
            }
        }
        Ok(false)
    }
}

pub fn run(args : Vec<String>) {
    //TODO pass &str
    //TODO handle parse errors
    let host_and_port : Vec<&str> = args[1].split(":").collect();
    let config = ConnectionConfig {
        host: String::from(host_and_port[0]),
        port: u16::from_str(host_and_port[1]).unwrap(),
        benchmark: args[args.len() - 1] == "--benchmark"
    };

    gtk_frontend::run(config);
}

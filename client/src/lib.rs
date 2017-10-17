#![feature(conservative_impl_trait)]
#![feature(const_fn)]
#![feature(drop_types_in_const)]

extern crate gtk;
extern crate gdk;
extern crate glib;
extern crate gdk_pixbuf;
#[macro_use]
extern crate derivative;

use std::io;
//use std::io::Read;
//use std::io::{Read,Write};
use std::net::TcpStream;
use std::sync::mpsc;

mod presentation;
use presentation::gtk as gtk_frontend;

pub mod protocol;

use protocol::rfb;
use protocol::parsing::io_input::SharedBuf;
use protocol::parsing::Packet;
use std::cell::RefCell;
use std::sync::{Arc,RwLock,RwLockWriteGuard};
use std::time::{Duration,Instant};

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
//[0, 0, 255, 0, 0, 0, 255, 0, 0, 0]
//-> 0x00ff0000 -> red: 0x00ff & 0xff

struct ViewPixelFormat {
    bytes_per_pixel : usize
}
const VIEW_PIXEL_FORMAT : ViewPixelFormat = ViewPixelFormat {
    bytes_per_pixel: 3
};

pub struct ConnectionConfig {
    pub host_and_port : String,
    pub benchmark : bool
}

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
#[derive(Clone, Copy)]
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
    pub fn no_of_pixels(&self) -> usize {
        (self.width as usize) * (self.height as usize)
    }
}
type SharedFbData = Arc<RwLock<Vec<u8>>>;
pub enum ProtocolEvent {
    ChangeFbSize(FbSize),
    UpdateFramebuffer(SharedFbData, FbSize)
}
pub trait View {
    fn change_fb_size_to(&self, size : FbSize) {
        self.handle_event(ProtocolEvent::ChangeFbSize(size));
    }
    fn update_framebuffer(&self, new_fb : SharedFbData, size : FbSize) {
        self.handle_event(ProtocolEvent::UpdateFramebuffer(new_fb, size));
    }

    fn handle_event(&self, event : ProtocolEvent);
    fn get_events(&self) -> &mpsc::Receiver<GuiEvent>;
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
                  now.duration_since(self.most_recent_instant));
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

fn parse_packet<T>(buffer : &SharedBuf, input : &TcpStream) 
    -> Result<T, MainError>
    where T : Packet
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

fn write_packet<T>(packet : T, output : &mut TcpStream)
    -> Result<(), MainError>
    where T : Packet
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
    let socket = match TcpStream::connect(&config.host_and_port) {
        Ok(s) => s,
        Err(err) => {
            //TODO implement From and then Display for MainError
            return Err(MainError(format!("Could not connect to {}: {}", 
                                         config.host_and_port, err)));
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

struct Framebuffer {
    data : SharedFbData,
    size : FbSize
}
impl Framebuffer {
    fn new() -> Self {
        Self {
            data: Arc::new(RwLock::new(Vec::new())),
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
        self.data.write().unwrap().resize(new_len, gray);
    }

    fn size(&self) -> FbSize {
        self.size
    }

    fn set_pixel(&self, data : &mut RwLockWriteGuard<Vec<u8>>, 
                 x : usize, y : usize,
                 r : u8, g : u8, b : u8) {
        let stride = VIEW_PIXEL_FORMAT.bytes_per_pixel 
            * (self.size.width as usize);
        let pos = y * stride + x * VIEW_PIXEL_FORMAT.bytes_per_pixel;
        data[pos] = r;
        data[pos + 1] = g;
        data[pos + 2] = b;
    }
}

struct RfbConnection<V : View> {
    config : ConnectionConfig,
    socket : TcpStream,
    view : V,
    buffer : SharedBuf,
    framebuffer : Framebuffer
}
impl<V : View> RfbConnection<V> {
    fn new(config : ConnectionConfig, socket : TcpStream, view : V) 
        -> Self
    {
        let buffer = RefCell::new(Vec::new());
        Self {
            config: config,
            socket: socket,
            view: view,
            buffer: buffer,
            framebuffer: Framebuffer::new()
        }
    }

    fn parse_packet<T>(&self) -> Result<T, MainError>
        where T : Packet
    {
        parse_packet::<T>(&self.buffer, &self.socket)
    }

    fn write_packet<T>(&mut self, packet : T) -> Result<(), MainError>
        where T : Packet
    {
        write_packet(packet, &mut self.socket)
    }

    fn resize_fb(&mut self, new_size : FbSize) {
        self.view.change_fb_size_to(new_size);
        self.framebuffer.resize(new_size);
    }

    fn fb_size(&self) -> FbSize {
        self.framebuffer.size()
    }

    fn send_fb_update_request(&mut self) -> Result<(), MainError> {
        let request = rfb::FramebufferUpdateRequest {
            incremental: false,
            x: 0,
            y: 0,
            width: self.fb_size().width,
            height: self.fb_size().height
        };
        self.write_packet(rfb::ClientToServer::FramebufferUpdateRequest(
                request))
    }

    fn handle(&mut self) -> Result<(), MainError>  {
        let server_address = self.socket.peer_addr().unwrap();

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
            shared: false
        })?;

        let server_init = self.parse_packet::<rfb::ServerInit>()?;
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
        self.resize_fb(FbSize::new(server_init.width, server_init.height));

        //TODO ClientToServer/ServerToClient main part of protocol into own function

        //TODO keep button mask as state
        //note: TurboVNC then uses a protocol writer with a mutex
        
        self.write_packet(rfb::ClientToServer::SetEncodings(rfb::SetEncodings {
            encodings: vec![
                rfb::ENCODING_RAW,
    //            rfb::ENCODING_TIGHT,
                rfb::ENCODING_DESKTOP_SIZE]
        }))?;

        self.write_packet(rfb::ClientToServer::SetPixelFormat(
                rfb::SetPixelFormat {
                    format: PIXEL_FORMAT.clone()
                }
        ))?;

        //handle_rfb_main_part(
        
    //    write_packet(rfb::ClientToServer::PointerEvent(rfb::PointerEvent {
    //        mask: 1, //button 1 (button 3 == 4)
    //        x: 5,
    //        y: 7
    //    }), &mut self.socket)?;

        //TODO non-blocking select between GUI and server? (use try_recv for mpsc)
        //TODO first: decode everything here, then: use worker threads and *benchmark this*!

        self.send_fb_update_request()?;
        
        let mut in_1_second = Instant::now() + Duration::from_secs(1);
        let mut fps : u32 = 0;
        let mut time_spent_waiting = Duration::from_secs(0);
//        let mut stopwatch = Stopwatch::new();
        let mut stopwatch = NullStopwatch::new();
        loop {
            stopwatch.take_measurement("start");
            while let Ok(event) = self.view.get_events().try_recv() {
                match event {
                    //TODO update button mask as state
                    GuiEvent::Pointer { state, mask: _, x, y } => {
                        self.write_packet(rfb::ClientToServer::PointerEvent(
                                rfb::PointerEvent {
                                    mask: state,
                                    x: x as u16, //TODO clamp
                                    y: y as u16
                                }))?;
                    },
                    _ => { }
                }
            }

            stopwatch.take_measurement("before request");
            //TODO do not block when waiting for the FramebufferUpdate

            stopwatch.take_measurement("waiting for update");
            let _raw_rect_size = 5;
            let before_waiting_for_fb_update = Instant::now();
            let server_packet = self.parse_packet::<rfb::ServerToClient>()?;
            time_spent_waiting += Instant::now().duration_since(
                before_waiting_for_fb_update);
            match server_packet {
                rfb::ServerToClient::FramebufferUpdate(update) => {
                    self.send_fb_update_request()?;
//                    eprintln!("received[{}] ‘{:?}’", server_address, update);
                    for _ in 0..update.no_of_rectangles {
                        let rectangle = parse_packet::<rfb::Rectangle>(
                            &self.buffer, &self.socket)?;
//                        eprintln!("received[{}] ‘{:?}’", server_address, rectangle);
                        match rectangle.payload {
                            //TODO refactor to e.g. rfb::rect::Raw with a pub type
                            //TODO test should_request_the_full_framebuffer_after_initialization and
                            //check if handled correctly
                            //next:
                            //x. LazyVec -> make compile (minus skip and change_fb_size and errors)
                            //x. implement change_fb_size
                            //x. pass walking skeleton
                            //x. write benchmark, first without any pixel data, just a loop
                            //5. write test above as end-to-end test, take a screenshot. Write the
                            //   benchmark while passing this test.
                            //6. implement skip?, handle errors for manual I/O operations (see TODO above)
                            //7. implement Tight encoding
                            //8. implement JPEG encoding (is Tight?)
                            rfb::RectanglePayload::RawRectangle(_) => {
                                //TODO implement
                                //TODO implement a skip function
                                //TODO handle error
                                
                                //TODO re-use buffer
    //                            assert_eq!(rectangle.width, fb_size.width);
    //                            assert_eq!(rectangle.height, fb_size.height);
                                let size = ((PIXEL_FORMAT
                                            .bits_per_pixel as usize) / 8)
                                    * (rectangle.width as usize)
                                    * (rectangle.height as usize);
                                let size = size as usize;
                                let mut bytes = Vec::with_capacity(size);
                                unsafe {
                                    bytes.set_len(size);
                                }
//                                eprintln!("received[{}] ‘{:?}’", server_address, rectangle);
                                let start = Instant::now();
                                ::std::io::Read::read_exact(&mut self.socket, &mut bytes[..])?;
                                time_spent_waiting += Instant::now().duration_since(start);
    //                            let framebuffer_size = 
    //                                VIEW_PIXEL_FORMAT.bytes_per_pixel
    //                                * (rectangle.width as usize)
    //                                * (rectangle.height as usize);
    //                            let framebuffer = Vec::with_capacity(framebuffer_size);
                                
                                //TODO convert in parser?
                                let rect_x = rectangle.x as usize;
                                let rect_y = rectangle.y as usize;
                                let width = rectangle.width as usize;
                                let height = rectangle.height as usize;

                                let mut fb_data = self.framebuffer.data.write().unwrap();
                                let mut i = 0;
                                for y in 0..height {
                                    for x in 0..width {
                                        let byte_pos = i * PIXEL_FORMAT_BYTES_PER_PIXEL;
                                        let bgra = &bytes[byte_pos..];
                                        self.framebuffer.set_pixel(
                                            &mut fb_data,
                                            x + rect_x,
                                            y + rect_y,
                                            bgra[2],
                                            bgra[1],
                                            bgra[0]
                                        );
                                        i += 1;
                                    }
                                }

    //                            eprintln!("[{}] raw rect: {:?}", server_address,
    //                                      &bytes[0..10]);
    //                            unimplemented!()
                            },
                            rfb::RectanglePayload::TightRectangle(_tight) => {
    //                            ::std::io::Read::read_exact(&mut socket, &mut bytes[..]).unwrap();
    //                            eprintln!("received[{}] tight: ‘{:?}’", server_address, byte);
                                unimplemented!()
                            },
                            rfb::RectanglePayload::DesktopSizeRectangle(_) => {
                                eprintln!("received[{}] ‘{:?}’", server_address, rectangle);
                                self.resize_fb(FbSize::new(
                                        rectangle.width, rectangle.height));
                            }
                        }
                    }
                    //TODO assert that frambuffer update affected whole fb?
                    if self.config.benchmark {
                        fps += 1;
                        if Instant::now() >= in_1_second {
                            let fps_without_waiting = 
                                (fps as f64)
                                / (1.0 - in_seconds(time_spent_waiting));
                            let fps_without_waiting = fps_without_waiting.round();
                            println!("{} {}", fps, fps_without_waiting);
                            ::std::io::Write::flush(
                                &mut ::std::io::stdout()).unwrap();
                            fps = 0;
                            time_spent_waiting = Duration::from_secs(0);
                            in_1_second += Duration::from_secs(1);
                        }
                    }

                    stopwatch.take_measurement("updating framebuffer");
                    let fb_data_reference = &self.framebuffer.data;
                    self.view.update_framebuffer(fb_data_reference.clone(), 
                                                 self.fb_size());
                    stopwatch.take_measurement("updated framebuffer");
                }, //frambuffer update (TODO move to func)
                _ => { }
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
}

pub fn run(args : Vec<String>) {
    //TODO pass &str
    let config = ConnectionConfig {
        host_and_port: args[1].clone(),
        benchmark: args[args.len() - 1] == "--benchmark"
    };

    gtk_frontend::run(config);
}

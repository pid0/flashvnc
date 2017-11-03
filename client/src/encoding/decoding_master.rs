use ::{SharedFb,FbSize,MainError,Rgb,PIXEL_FORMAT_BYTES_PER_PIXEL,Framebuffer,
       FbSlice,FbRawParts,FbPointerSlice,Cursor,CursorSize,Hotspot};
use std::io;
use protocol::rfb;

use std::sync::{Arc,Mutex,RwLockWriteGuard};
use std::cell::RefCell;

use tight::ZlibDecoder;
use tight::jpeg::Decoder as JpegDecoder;

use infrastructure::thread_pool::{ThreadPool,Future,FutureCollection};
use infrastructure::BitBuffer;

pub enum EncodingMethod {
    RawBgra(Vec<u8>),
    CopyFilter(TightData),
    PaletteFilter(Vec<Rgb>, TightData),
    Jpeg(Vec<u8>),
    Fill(Rgb),
    CursorBgrx {
        pixels: Vec<u8>,
        bitmask: Vec<u8>
    }
}

pub enum TightData {
    UncompressedRgb(Vec<u8>),
    CompressedRgb {
        stream_no : usize,
        bytes : Vec<u8>
    }
}

pub enum DecodingJob {
    ResetZlib(usize),
    Rect {
        bounds : Bounds,
        method : EncodingMethod
    }
}
impl DecodingJob {
    pub fn rect_from_rfb(rect : &rfb::Rectangle, method : EncodingMethod) -> Self {
        DecodingJob::Rect {
            bounds: Bounds::new(rect.x, rect.y,
                                FbSize::new(rect.width, rect.height)),
            method: method
        }
    }
}

//TODO use in other places
//#[derive(Clone, Copy)]
pub struct Bounds {
    x : usize,
    y : usize,
    size : FbSize
}
impl Bounds {
    pub fn new(x : usize, y : usize, size : FbSize) -> Self {
        Self {
            x: x,
            y: y,
            size: size
        }
    }

    fn width(&self) -> usize {
        self.size.width
    }
    fn height(&self) -> usize {
        self.size.height
    }
}

pub struct FbLock<'a> {
    lock : RwLockWriteGuard<'a, Framebuffer>
}
impl<'a> FbLock<'a> {
    pub fn get_raw_parts(&self) -> FbRawParts {
        self.lock.get_raw_parts()
    }
}

struct Decoders {
    jpeg_decoder : JpegDecoder,
    zlib_decoder : ZlibDecoder
}
impl Decoders {
    fn new() -> Self {
        Self {
            jpeg_decoder: JpegDecoder::new(),
            zlib_decoder: ZlibDecoder::new()
        }
    }
}

type SharedCursor = Arc<Mutex<Cursor>>;
type State = (Decoders, SharedCursor);

type DecoderPool = ThreadPool<State>;

pub struct DecodingMaster {
    framebuffer : SharedFb,
    general_decoders : DecoderPool,
    zlib_decoders : [DecoderPool; 4],
    futures : RefCell<Vec<Future<MainError>>>
}
impl DecodingMaster {
    pub fn new(fb : SharedFb, cursor : SharedCursor) -> Self {
        let cursor_clone_1 = cursor.clone();
        let cursor_clone_2 = cursor.clone();
        let cursor_clone_3 = cursor.clone();
        let cursor_clone_4 = cursor.clone();

        let general_decoders = ThreadPool::new(
            "general-decoder",
            4, move || (Decoders::new(), cursor.clone()));
        let zlib_decoders = [
            ThreadPool::new("zlib-decoder-1", 1, move ||
                            (Decoders::new(), cursor_clone_1.clone())),
            ThreadPool::new("zlib-decoder-2", 1, move ||
                            (Decoders::new(), cursor_clone_2.clone())),
            ThreadPool::new("zlib-decoder-3", 1, move ||
                            (Decoders::new(), cursor_clone_3.clone())),
            ThreadPool::new("zlib-decoder-4", 1, move ||
                            (Decoders::new(), cursor_clone_4.clone()))];

        Self {
            framebuffer: fb,
            general_decoders: general_decoders,
            zlib_decoders: zlib_decoders,
            futures: RefCell::new(Vec::with_capacity(20))
        }
    }

    pub fn lock_framebuffer(&self) -> FbLock {
        FbLock {
            lock: self.framebuffer.write().unwrap()
        }
    }

    fn spawn_job(&self, pool : &DecoderPool, fb_lock : &FbLock,
                 bounds : Bounds, method : EncodingMethod) {
        let fb_raw_parts = fb_lock.get_raw_parts();
        self.futures.borrow_mut().push(pool.spawn_fn(
                move |&mut (ref mut decoders, ref mut cursor)| { 
                    decode(
                        fb_raw_parts, &cursor,
                        &mut decoders.zlib_decoder,
                        &mut decoders.jpeg_decoder,
                        bounds, method) 
                }));
    }

    pub fn accept(&self, fb_lock : &FbLock, job : DecodingJob) {
        use DecodingJob::*;
        use EncodingMethod::*;
        use TightData::*;

        match job {
            Rect { bounds, method } => {
                match method {
                    CopyFilter(CompressedRgb { stream_no, bytes: _ }) => {
                        self.spawn_job(&self.zlib_decoders[stream_no],
                                       fb_lock, bounds, method);
                    },
                    PaletteFilter(_, CompressedRgb { stream_no, bytes: _}) => {
                        self.spawn_job(&self.zlib_decoders[stream_no],
                                       fb_lock, bounds, method);
                    },
                    _ => {
                        self.spawn_job(&self.general_decoders,
                                       fb_lock, bounds, method);
                    }
                }
            },
            ResetZlib(stream_number) => {
                let pool = &self.zlib_decoders[stream_number];
                self.futures.borrow_mut().push(pool.spawn_fn(
                        |&mut (ref mut decoders, _)| { 
                            decoders.zlib_decoder.reset(); 
                            Ok(()) 
                        }));
            }
        }
    }

    pub fn finish(&self) -> FutureCollection<MainError> {
        let mut futures = self.futures.borrow_mut();
        let ret = FutureCollection::new(futures.drain(..).collect());
        ret
    }
}

fn decode(fb_parts : FbRawParts, cursor : &SharedCursor,
          zlib_decoder : &mut ZlibDecoder, jpeg_decoder : &mut JpegDecoder,
          bounds : Bounds,
          method : EncodingMethod) -> Result<(), MainError>
{
    use EncodingMethod::*;
    let mut fb = unsafe {
        FbPointerSlice::from_raw_parts(fb_parts)
    };

    match method {
        Fill(color) => {
            for y in 0..bounds.height() {
                for x in 0..bounds.width() {
                    fb.set_pixel(
                        x + bounds.x,
                        y + bounds.y,
                        color.r,
                        color.g,
                        color.b);
                }
            }
        },

        RawBgra(bytes) => {
            let mut i = 0;
            for y in 0..bounds.height() {
                for x in 0..bounds.width() {
                    let byte_pos = i * PIXEL_FORMAT_BYTES_PER_PIXEL;
                    let bgra = &bytes[byte_pos..];
                    fb.set_pixel(
                        x + bounds.x,
                        y + bounds.y,
                        bgra[2],
                        bgra[1],
                        bgra[0]
                    );
                    i += 1;
                }
            }
        },

        CopyFilter(data) => {
            let data = uncompress(zlib_decoder, &data)?;

            let mut i = 0;
//                let stride = TPIXEL_SIZE * rectangle.width;
            for y in 0..bounds.height() {
//                let line = &uncompressed[stride * y..];
//                framebuffer.set_line(
//                    rectangle.x,
//                    rectangle.width,
//                    y + rectangle.y,
//                    unsafe {
//                        std::slice::from_raw_parts(
//                            line.as_ptr() as *const Rgb,
//                            line.len())
//                    });

                for x in 0..bounds.width() {
                    fb.set_pixel(
                        x + bounds.x,
                        y + bounds.y,
                        data[i],
                        data[i + 1],
                        data[i + 2]);
                    i += 3;
                }
            }
        },

        PaletteFilter(colors, data) => {
            let data = uncompress(zlib_decoder, &data)?;

            if colors.len() == 2 {
                let mut bits = BitBuffer::new(data);
                for y in 0..bounds.height() {
                    bits.next_byte();
                    for x in 0..bounds.width() {
                        let color = &colors[bits.next() as usize];
                        fb.set_pixel(
                            x + bounds.x,
                            y + bounds.y,
                            color.r,
                            color.g,
                            color.b);
                    }
                }
            } else {
                let mut i = 0;
                for y in 0..bounds.height() {
                    for x in 0..bounds.width() {
                        //bounds check?
                        let color = &colors[data[i] as usize];
                        fb.set_pixel(
                            x + bounds.x,
                            y + bounds.y,
                            color.r,
                            color.g,
                            color.b);
                        i += 1;
                    }
                }
            }
        },

        Jpeg(data) => {
            let mut image = jpeg_decoder.decode(&data[..]).unwrap();

            //TODO return ParseErrors
//                assert_eq!(image.width, bounds.width());
//                assert_eq!(image.height, bounds.height());

            let mut y = 0;
            while image.next_line().unwrap() {
                let line = image.current_line();
                fb.set_line(
                    bounds.x,
                    bounds.width(),
                    y + bounds.y,
                    line);
                y += 1;
            }
        },

        CursorBgrx { pixels, bitmask } => {
            let mut rgba = Vec::with_capacity(pixels.len());
            let mut bits = BitBuffer::new(&bitmask[..]);
            let mut i = 0;
            for _ in 0..bounds.height() {
                for _ in 0..bounds.width() {
                    rgba.push(pixels[i + 2]);
                    rgba.push(pixels[i + 1]);
                    rgba.push(pixels[i + 0]);
                    rgba.push(bits.next() * 255);
                    i += PIXEL_FORMAT_BYTES_PER_PIXEL;
                }
                bits.next_byte();
            }
            let mut cursor = cursor.lock().unwrap();
            cursor.change_data(rgba, 
                               CursorSize(bounds.width(), bounds.height()),
                               Hotspot(bounds.x, bounds.y));
        }
    }

    Ok(())
}

fn uncompress<'a, 'b : 'a>(decoder : &'b mut ZlibDecoder, data : &'a TightData)
    -> io::Result<&'a [u8]>
{
    use TightData::*;
    Ok(match *data {
        UncompressedRgb(ref bytes) => &bytes[..],
        CompressedRgb { stream_no: _, ref bytes } => {
            decoder.decode(&bytes[..])?
        }
    })
}

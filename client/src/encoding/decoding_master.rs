use ::{SharedFb,FbSize,MainError,Rgb,PIXEL_FORMAT_BYTES_PER_PIXEL,Framebuffer,
       FbSlice,FbRawParts,FbPointerSlice};
use std::io;
use protocol::rfb;

use std::sync::RwLockWriteGuard;
use std::cell::RefCell;

use tight::ZlibDecoder;
use tight::jpeg::Decoder as JpegDecoder;

use infrastructure::thread_pool::{ThreadPool,Future};
use infrastructure::thread_pool::Error as ThreadPoolError;

type JobError = ThreadPoolError<MainError>;

pub enum EncodingMethod {
    RawBgra(Vec<u8>),
    CopyFilter(TightData),
    PaletteFilter(Vec<Rgb>, TightData),
    Jpeg(Vec<u8>),
    Fill(Rgb)
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
    fn get_raw_parts(&self) -> FbRawParts {
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

type DecoderPool = ThreadPool<Decoders>;

pub struct DecodingMaster {
    framebuffer : SharedFb,
    general_decoders : DecoderPool,
    zlib_decoders : [DecoderPool; 4],
    futures : RefCell<Vec<Future<MainError>>>
}
impl DecodingMaster {
    pub fn new(fb : SharedFb) -> Self {
        let general_decoders = ThreadPool::new(
            "general-decoder",
            4, || Decoders::new());
        let zlib_decoders = [
            ThreadPool::new("zlib-decoder-1", 1, || Decoders::new()),
            ThreadPool::new("zlib-decoder-2", 1, || Decoders::new()),
            ThreadPool::new("zlib-decoder-3", 1, || Decoders::new()),
            ThreadPool::new("zlib-decoder-4", 1, || Decoders::new())];

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
        self.futures.borrow_mut().push(pool.spawn_fn(move |decoders| {
            decode(
                fb_raw_parts,
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
                self.futures.borrow_mut().push(pool.spawn_fn(|decoders| {
                    decoders.zlib_decoder.reset();
                    Ok(())
                }));
            }
        }
    }

    pub fn finish(&self, _lock : FbLock) -> Result<(), Vec<JobError>> {
        let mut errors = Vec::new();
        let mut futures = self.futures.borrow_mut();
        for future in futures.drain(..) {
            if let Err(err) = future.wait() {
                errors.push(err);
            }
        }
        if errors.len() == 0 {
            Ok(())
        } else {
            Err(errors)
        }
    }
}

fn decode(fb_parts : FbRawParts, zlib_decoder : &mut ZlibDecoder,
          jpeg_decoder : &mut JpegDecoder,
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
                let mut i = 0;
                for y in 0..bounds.height() {
                    let mut bit = 0;
                    let mut byte = 0;
                    for x in 0..bounds.width() {
                        if bit == 0 {
                            byte = data[i as usize];
                            i += 1;
                        }

                        let color = &colors[((byte & 0x80) >> 7) as usize];
                        byte <<= 1;
                        fb.set_pixel(
                            x + bounds.x,
                            y + bounds.y,
                            color.r,
                            color.g,
                            color.b);
                        bit += 1;
                        if bit == 8 {
                            bit = 0;
                        }
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

//struct ZlibDecoders {
//    decoders : [ZlibDecoder; 4]
//}
//impl ZlibDecoders {
//    fn new() -> Self {
//        Self {
//            decoders: zlib_decoders
//        }
//    }
//
//    fn reset(&mut self, stream_number : usize) {
//        self.decoders[stream_number].reset();
//    }
//
//    fn uncompress<'a, 'b : 'a>(&'b mut self, data : &'a TightData)
//        -> io::Result<&'a [u8]>
//    {
//        Ok(match *data {
//            TightData::UncompressedRgb(ref bytes) => &bytes[..],
//            TightData::CompressedRgb { stream_no, ref bytes } => {
//                self.decoders[stream_no].decode(&bytes[..])?
//            }
//        })
//    }
//}

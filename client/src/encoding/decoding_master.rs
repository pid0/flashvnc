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

use ::{SharedFb,FbSize,MainError,Bgrx,PIXEL_FORMAT_BYTES_PER_PIXEL,
       FbSlice,FbAccess,Cursor,CursorSize,Hotspot,PixelFormat,
       FB_PIXEL_FORMAT};
use std::io;
use protocol::rfb;

use std::sync::{Arc,Mutex};
use std::cell::RefCell;
use std::ptr;

use tight::ZlibDecoder;
use tight::jpeg::Decoder as JpegDecoder;

use infrastructure::thread_pool::{ThreadPool,Future,FutureCollection};
use infrastructure::BitBuffer;

pub enum EncodingMethod {
    RawBgra(Vec<u8>),
    CopyFilter(TightData),
    PaletteFilter(Vec<Bgrx>, TightData),
    Jpeg(Vec<u8>),
    Fill(Bgrx),
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

#[derive(Clone, Copy)]
struct BufPointer(*const u8);
unsafe impl Send for BufPointer { }
#[derive(Clone, Copy)]
struct MutBufPointer(*mut u8);
unsafe impl Send for MutBufPointer { }

type SharedCursor = Arc<Mutex<Cursor>>;
type State = (Decoders, SharedFb, SharedCursor);

type DecoderPool = ThreadPool<State>;

pub struct DecodingMaster {
    general_decoders : DecoderPool,
    zlib_decoders : [DecoderPool; 4],
    futures : RefCell<Vec<Future<MainError>>>,
    framebuffer : SharedFb,
    no_of_threads : usize
}
impl DecodingMaster {
    pub fn new(fb : SharedFb, cursor : SharedCursor) -> Self {
        let cursor_clone_1 = cursor.clone();
        let cursor_clone_2 = cursor.clone();
        let cursor_clone_3 = cursor.clone();
        let cursor_clone_4 = cursor.clone();
        let fb_clone = fb.clone();
        let fb_clone_1 = fb.clone();
        let fb_clone_2 = fb.clone();
        let fb_clone_3 = fb.clone();
        let fb_clone_4 = fb.clone();
        let no_of_threads = 4;

        let general_decoders = ThreadPool::new(
            "general-decoder", no_of_threads,
            move || (Decoders::new(), fb_clone.clone(), cursor.clone()));
        let zlib_decoders = [
            ThreadPool::new("zlib-decoder-1", 1, move ||
                            (Decoders::new(), fb_clone_1.clone(), 
                             cursor_clone_1.clone())),
            ThreadPool::new("zlib-decoder-2", 1, move ||
                            (Decoders::new(), fb_clone_2.clone(),
                            cursor_clone_2.clone())),
            ThreadPool::new("zlib-decoder-3", 1, move ||
                            (Decoders::new(), fb_clone_3.clone(),
                            cursor_clone_3.clone())),
            ThreadPool::new("zlib-decoder-4", 1, move ||
                            (Decoders::new(), fb_clone_4.clone(),
                            cursor_clone_4.clone()))];

        Self {
            general_decoders: general_decoders,
            zlib_decoders: zlib_decoders,
            futures: RefCell::new(Vec::with_capacity(20)),
            framebuffer: fb,
            no_of_threads: no_of_threads
        }
    }

    fn spawn_job(&self, pool : &DecoderPool,
                 bounds : Bounds, method : EncodingMethod) {
        self.futures.borrow_mut().push(pool.spawn_fn(
                move |&mut (ref mut decoders, ref fb, ref mut cursor)| {
                    decode(
                        fb, &cursor,
                        &mut decoders.zlib_decoder,
                        &mut decoders.jpeg_decoder,
                        bounds, method) 
                }));
    }

    pub fn accept(&self, job : DecodingJob) {
        use DecodingJob::*;
        use EncodingMethod::*;
        use TightData::*;

        match job {
            Rect { bounds, method } => {
                match method {
                    CopyFilter(CompressedRgb { stream_no, bytes: _ }) => {
                        self.spawn_job(&self.zlib_decoders[stream_no],
                                       bounds, method);
                    },
                    PaletteFilter(_, CompressedRgb { stream_no, bytes: _}) => {
                        self.spawn_job(&self.zlib_decoders[stream_no],
                                       bounds, method);
                    },
                    _ => {
                        self.spawn_job(&self.general_decoders,
                                       bounds, method);
                    }
                }
            },
            ResetZlib(stream_number) => {
                let pool = &self.zlib_decoders[stream_number];
                self.futures.borrow_mut().push(pool.spawn_fn(
                        |&mut (ref mut decoders, _, _)| { 
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

    pub fn convert_or_copy_fb(&mut self, dest_format : PixelFormat)
        -> (Vec<u8>, FbSize)
    {
        let fb = self.framebuffer.lock(FbAccess::Reading);
        let size = fb.size();

        let pixels = size.no_of_pixels();
        let pixels_for_one = pixels / self.no_of_threads;
        let surplus = pixels % self.no_of_threads;
        let pixels_for_last = pixels_for_one + surplus;

        let dst_len = pixels * dest_format.bytes_per_pixel();
        let mut dst = Vec::with_capacity(dst_len);
        unsafe {
            dst.set_len(dst_len);
        }
        let dst_ptr = dst.as_mut_ptr();
        let src_ptr = fb.data().as_ptr();

        let func = match dest_format {
            PixelFormat::NativeBgrx => copy,
            PixelFormat::Rgb => bgrx_to_rgb
        };

        let futures : Vec<Future<()>> = (0..self.no_of_threads).map(|i| {
            let src_offset = i * pixels_for_one
                * FB_PIXEL_FORMAT.bytes_per_pixel;
            let dst_offset = i * pixels_for_one * dest_format.bytes_per_pixel();
            let src_start = BufPointer(unsafe { src_ptr.offset(
                    src_offset as isize) });
            let dst_start = MutBufPointer(unsafe { dst_ptr.offset(
                    dst_offset as isize) });
            let pixels = if i == self.no_of_threads - 1 {
                pixels_for_last
            } else {
                pixels_for_one
            };
            self.general_decoders.spawn_fn(move |_| {
                func(dst_start.0, src_start.0, pixels);
                Ok(())
            })
        }).collect();

        for future in futures {
            future.wait().unwrap();
        }

        (dst, size)
    }
}

fn bgrx_to_rgb(dst : *mut u8, src : *const u8, pixels : usize) {
    unsafe {
        let mut src_i = 0;
        let mut dst_i = 0;
        for _ in 0..pixels {
            *dst.offset(dst_i + 0) = *src.offset(src_i + 2);
            *dst.offset(dst_i + 1) = *src.offset(src_i + 1);
            *dst.offset(dst_i + 2) = *src.offset(src_i + 0);
            src_i += 4;
            dst_i += 3;
        }
    }
}
fn copy(dst : *mut u8, src : *const u8, pixels : usize) {
    unsafe {
        ptr::copy_nonoverlapping(
            src, dst, pixels * FB_PIXEL_FORMAT.bytes_per_pixel);
    }
}

fn decode(fb : &SharedFb, cursor : &SharedCursor,
          zlib_decoder : &mut ZlibDecoder, jpeg_decoder : &mut JpegDecoder,
          bounds : Bounds,
          method : EncodingMethod) -> Result<(), MainError>
{
    use EncodingMethod::*;
    let mut fb = fb.lock(FbAccess::Decoding);

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

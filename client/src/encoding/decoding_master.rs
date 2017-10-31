use ::{SharedFb,FbSize,MainError,Rgb,PIXEL_FORMAT_BYTES_PER_PIXEL};
use std::io;
use protocol::rfb;

use tight::ZlibDecoder;
use tight::jpeg::Decoder as JpegDecoder;

pub enum EncodingMethod {
    RawBgra(Vec<u8>),
    CopyFilter(TightData),
    PaletteFilter(Vec<Rgb>, TightData),
    Jpeg(Vec<u8>)
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
        x : usize,
        y : usize,
        size : FbSize,
        method : EncodingMethod
    }
}
impl DecodingJob {
    pub fn rect_from_rfb(rect : &rfb::Rectangle, method : EncodingMethod) -> Self {
        DecodingJob::Rect {
            x: rect.x,
            y: rect.y,
            size: FbSize::new(rect.width, rect.height),
            method: method
        }
    }
}

pub struct DecodingMaster {
    framebuffer : SharedFb,
    zlib_decoders : ZlibDecoders,
    jpeg_decoder : JpegDecoder
}
impl DecodingMaster {
    pub fn new(fb : SharedFb) -> Self {
        Self {
            framebuffer: fb,
            zlib_decoders: ZlibDecoders::new(),
            jpeg_decoder: JpegDecoder::new()
        }
    }

    pub fn accept(&mut self, job : DecodingJob) -> Result<(), MainError> {
        use DecodingJob::*;

        match job {
            Rect { x, y, size, method } => {
                self.decode(x, y, size, method)?;
            },
            ResetZlib(stream_number) => {
                self.zlib_decoders.reset(stream_number);
            }
        }
        Ok(())
    }

    fn decode(&mut self, start_x : usize, start_y : usize, size : FbSize,
              method : EncodingMethod) -> Result<(), MainError>
    {
        use EncodingMethod::*;

        match method {
            RawBgra(bytes) => {
                let mut fb = self.framebuffer.write().unwrap();
                let mut i = 0;
                for y in 0..size.height {
                    for x in 0..size.width {
                        let byte_pos = i * PIXEL_FORMAT_BYTES_PER_PIXEL;
                        let bgra = &bytes[byte_pos..];
                        fb.set_pixel(
                            x + start_x,
                            y + start_y,
                            bgra[2],
                            bgra[1],
                            bgra[0]
                        );
                        i += 1;
                    }
                }
            },

            //TODO refactor framebuffer write lock?
            CopyFilter(data) => {
                let data = self.zlib_decoders.uncompress(&data)?;

                let mut fb = self.framebuffer.write().unwrap();
                let mut i = 0;
//                let stride = TPIXEL_SIZE * rectangle.width;
                for y in 0..size.height {
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

                    for x in 0..size.width {
                        fb.set_pixel(
                            x + start_x,
                            y + start_y,
                            data[i],
                            data[i + 1],
                            data[i + 2]);
                        i += 3;
                    }
                }
            },

            PaletteFilter(colors, data) => {
                let mut fb = self.framebuffer.write().unwrap();
                let data = self.zlib_decoders.uncompress(&data)?;

                if colors.len() == 2 {
                    let mut i = 0;
                    for y in 0..size.height {
                        let mut bit = 0;
                        let mut byte = 0;
                        for x in 0..size.width {
                            if bit == 0 {
                                byte = data[i as usize];
                                i += 1;
                            }

                            let color = &colors[((byte & 0x80) >> 7) as usize];
                            byte <<= 1;
                            fb.set_pixel(
                                x + start_x,
                                y + start_y,
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
                    for y in 0..size.height {
                        for x in 0..size.width {
                            //bounds check?
                            let color = &colors[data[i] as usize];
                            fb.set_pixel(
                                x + start_x,
                                y + start_y,
                                color.r,
                                color.g,
                                color.b);
                            i += 1;
                        }
                    }
                }
            },

            Jpeg(data) => {
                let mut image = self.jpeg_decoder.decode(&data[..]).unwrap();
                let mut fb = self.framebuffer.write().unwrap();

                //TODO return ParseErrors
//                assert_eq!(image.width, size.width);
//                assert_eq!(image.height, size.height);

                let mut y = 0;
                while image.next_line().unwrap() {
                    let line = image.current_line();
                    fb.set_line(
                        start_x,
                        size.width,
                        y + start_y,
                        line);
                    y += 1;
                }
            }
        }

        Ok(())
    }
}

struct ZlibDecoders {
    decoders : [ZlibDecoder; 4]
}
impl ZlibDecoders {
    fn new() -> Self {
        let zlib_decoders = [
            ZlibDecoder::new(),
            ZlibDecoder::new(),
            ZlibDecoder::new(),
            ZlibDecoder::new()];
        Self {
            decoders: zlib_decoders
        }
    }

    fn reset(&mut self, stream_number : usize) {
        self.decoders[stream_number].reset();
    }

    fn uncompress<'a, 'b : 'a>(&'b mut self, data : &'a TightData)
        -> io::Result<&'a [u8]>
    {
        Ok(match *data {
            TightData::UncompressedRgb(ref bytes) => &bytes[..],
            TightData::CompressedRgb { stream_no, ref bytes } => {
                self.decoders[stream_no].decode(&bytes[..])?
            }
        })
    }
}

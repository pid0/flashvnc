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

use flate2::write::ZlibDecoder as IoZlibDecoder;
use std::io::{self,Write};

//use #[link(name = "jpeg")]?
//see glfw.rs
mod ffi {
}

pub struct ZlibDecoder {
    zlib : IoZlibDecoder<Vec<u8>>
}
impl ZlibDecoder {
    pub fn new() -> Self {
        Self {
            zlib: IoZlibDecoder::new(Vec::new())
        }
    }

    fn buffer(&mut self) -> &mut Vec<u8> {
        self.zlib.get_mut()
    }

    pub fn reset(&mut self) {
        let temp = Vec::new();
        let buffer = self.zlib.reset(temp).unwrap();
        self.zlib.reset(buffer).unwrap();
    }

    pub fn decode(&mut self, input : &[u8]) -> io::Result<&[u8]> {
        self.buffer().clear();
        self.zlib.write_all(input)?;
        self.zlib.flush()?;
        Ok(&self.buffer()[..])
    }
}

pub mod jpeg {
    use std::fmt::{self,Display};
    use ::Bgrx;
    use std::ffi::CString;
    use libc::{c_int,c_void,size_t,c_uchar,c_char};

    type DecoderState = *mut c_void;
    const JPGINT_OK : c_int = 0;
    const _JPGINT_ERROR : c_int = 1;

    extern "C" {
        fn jpgint_dec_new() -> DecoderState;
        //TODO test resumption after error
        //TODO benchmark restrict in C
        fn jpgint_dec_destroy(state : DecoderState);
        fn jpgint_dec_end(state : DecoderState);
        fn jpgint_dec_abort(state : DecoderState);
        fn jpgint_dec_start(state : DecoderState, src : *const c_uchar, 
                         len : size_t) -> c_int;
        fn jpgint_dec_get_width(state : DecoderState) -> size_t;
        fn jpgint_dec_get_height(state : DecoderState) -> size_t;
        fn jpgint_dec_next_line(state : DecoderState,
                             dest : *mut c_uchar) -> c_int;
        fn jpgint_get_error(state : DecoderState, dest : *mut c_char);
    }

    #[derive(Debug)]
    pub struct Error(String);
    impl Display for Error {
        fn fmt(&self, f : &mut fmt::Formatter) -> fmt::Result {
            write!(f, "{}", self.0)
        }
    }
    pub type Result<T> = ::std::result::Result<T, Error>;

    fn get_error(state : DecoderState) -> Error {
        let mut bytes : Vec<u8> = Vec::new();
        //TODO determine correct maximum length
        bytes.resize(100, 0x20);
        let c_string = CString::new(bytes).unwrap();
        let pointer = c_string.into_raw();
        let string;
        unsafe {
            jpgint_get_error(state, pointer);
            string = CString::from_raw(pointer).into_string().unwrap();
        }
        Error(string)
    }

    pub struct Decoder {
        state : DecoderState
    }
    impl Decoder {
        pub fn new() -> Self {
            Self {
                state: unsafe { jpgint_dec_new() }
            }
        }

        pub fn decode(&self, src : &[u8]) -> Result<DecodedImage> {
            let ret = unsafe {
                jpgint_dec_start(self.state, src.as_ptr(), src.len())
            };
            if ret != JPGINT_OK {
                return Err(get_error(self.state));
            }

            Ok(DecodedImage::new(
                self.state,
                unsafe { jpgint_dec_get_width(self.state) },
                unsafe { jpgint_dec_get_height(self.state) }))
        }
    }
    impl Drop for Decoder {
        fn drop(&mut self) {
            unsafe {
                jpgint_dec_destroy(self.state);
            }
        }
    }
    unsafe impl Send for Decoder { }

    pub struct DecodedImage {
        state : DecoderState,
        pub width : usize,
        height : usize,
        //TODO unit test failure
        //TODO benchmark decoder in extra test: make this a RefCell and the struct immutable so
        //that the compiler knows line stays the same
        //also benchmark: remove setjmps
        line_number : usize,
        line : Vec<Bgrx>
    }
    impl DecodedImage {
        fn new(state : DecoderState, width : usize, height : usize) -> Self {
            let mut line = Vec::with_capacity(width);
            unsafe {
                line.set_len(width);
            }
            Self {
                state: state,
                width: width,
                height: height,
                line_number: 0,
                //line_number: RefCell::new(0),
                line: line
            }
        }

        fn finished(&self) -> bool {
            self.line_number >= self.height
        }

        pub fn next_line(&mut self) -> Result<bool> {
            if self.finished() {
                return Ok(false);
            }

            let ret = unsafe {
                jpgint_dec_next_line(
                    self.state, 
                    self.line.as_mut_ptr() as *mut c_uchar)
            };
            if ret != JPGINT_OK {
                return Err(get_error(self.state));
            }

            self.line_number += 1;
            Ok(true)
        }

        pub fn current_line(&self) -> &[Bgrx] {
            &self.line[..]
        }
    }
    impl Drop for DecodedImage {
        fn drop(&mut self) {
            if self.finished() {
                unsafe {
                    jpgint_dec_end(self.state);
                }
            } else {
                //TODO test this
                unsafe {
                    jpgint_dec_abort(self.state);
                }
            }
        }
    }

    #[cfg(test)]
    mod the_jpeg_decoder {
        use super::*;
        use test::Bencher;

//        #[test]
//        fn should_decode_images() {
//            use std::fs::File;
//            use std::io::{Read,Write};
//            let mut input = Vec::new();
//            File::open("/tmp/in.jpg").unwrap().read_to_end(&mut input).unwrap();
//            let decoder = Decoder::new();
//
//            let mut output = File::create("/tmp/out.foo").unwrap();
//            let mut image = decoder.decode(&input[..]).unwrap();
//            let header = format!("P6\n{}\n{}\n255\n", image.width, image.height);
//            output.write_all(header.as_bytes()).unwrap();
//            while image.next_line().unwrap() {
//                let line = image.current_line();
//                for x in 0..image.width {
//                    output.write_all(&[line[x].r, line[x].g, line[x].b][..]).unwrap();
//                }
//            }
//        }

        #[test]
        fn should_decode_jfifs() {
            let decoder = Decoder::new();
            let mut image = decoder.decode(&ONE_BY_ONE_JPEG[..]).unwrap();
            assert_eq!(image.width, 1);
            assert_eq!(image.height, 1);

            let mut lines = 0;
            while image.next_line().unwrap() {
                let line = image.current_line();
                assert_eq!(line[0].r, 0);
                assert_eq!(line[0].g, 127);
                assert_eq!(line[0].b, 255);
                lines += 1;
            }
            assert_eq!(lines, 1);
        }

        #[test]
        fn should_return_an_error_upon_getting_invalid_input() {
            let decoder = Decoder::new();
            let error = match decoder.decode(&[0x0, 0x1][..]) {
                Ok(_) => panic!("no error"),
                Err(e) => e
            };
            assert!(format!("{}", error).contains("Not a JPEG"));
        }
        //TODO
//        fn should_be_able_to_resume_decoding_after_a_failed_attempt() {
//        fn should_return_an_error_if_bytes_are_missing() {

        #[bench]
        fn constant_overhead(b : &mut Bencher) {
            let decoder = Decoder::new();

            b.iter(|| {
                let mut image = decoder.decode(&ONE_BY_ONE_JPEG[..]).unwrap();
                image.next_line().unwrap();
                image
            });
        }

        const ONE_BY_ONE_JPEG : [u8; 633] = [
            0xff, 0xd8, 0xff, 0xe0, 0x00, 0x10, 0x4a, 0x46, 0x49, 0x46, 0x00, 0x01,
            0x01, 0x00, 0x00, 0x01, 0x00, 0x01, 0x00, 0x00, 0xff, 0xdb, 0x00, 0x43,
            0x00, 0x08, 0x06, 0x06, 0x07, 0x06, 0x05, 0x08, 0x07, 0x07, 0x07, 0x09,
            0x09, 0x08, 0x0a, 0x0c, 0x14, 0x0d, 0x0c, 0x0b, 0x0b, 0x0c, 0x19, 0x12,
            0x13, 0x0f, 0x14, 0x1d, 0x1a, 0x1f, 0x1e, 0x1d, 0x1a, 0x1c, 0x1c, 0x20,
            0x24, 0x2e, 0x27, 0x20, 0x22, 0x2c, 0x23, 0x1c, 0x1c, 0x28, 0x37, 0x29,
            0x2c, 0x30, 0x31, 0x34, 0x34, 0x34, 0x1f, 0x27, 0x39, 0x3d, 0x38, 0x32,
            0x3c, 0x2e, 0x33, 0x34, 0x32, 0xff, 0xdb, 0x00, 0x43, 0x01, 0x09, 0x09,
            0x09, 0x0c, 0x0b, 0x0c, 0x18, 0x0d, 0x0d, 0x18, 0x32, 0x21, 0x1c, 0x21,
            0x32, 0x32, 0x32, 0x32, 0x32, 0x32, 0x32, 0x32, 0x32, 0x32, 0x32, 0x32,
            0x32, 0x32, 0x32, 0x32, 0x32, 0x32, 0x32, 0x32, 0x32, 0x32, 0x32, 0x32,
            0x32, 0x32, 0x32, 0x32, 0x32, 0x32, 0x32, 0x32, 0x32, 0x32, 0x32, 0x32,
            0x32, 0x32, 0x32, 0x32, 0x32, 0x32, 0x32, 0x32, 0x32, 0x32, 0x32, 0x32,
            0x32, 0x32, 0xff, 0xc0, 0x00, 0x11, 0x08, 0x00, 0x01, 0x00, 0x01, 0x03,
            0x01, 0x22, 0x00, 0x02, 0x11, 0x01, 0x03, 0x11, 0x01, 0xff, 0xc4, 0x00,
            0x1f, 0x00, 0x00, 0x01, 0x05, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x02, 0x03, 0x04, 0x05,
            0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0xff, 0xc4, 0x00, 0xb5, 0x10, 0x00,
            0x02, 0x01, 0x03, 0x03, 0x02, 0x04, 0x03, 0x05, 0x05, 0x04, 0x04, 0x00,
            0x00, 0x01, 0x7d, 0x01, 0x02, 0x03, 0x00, 0x04, 0x11, 0x05, 0x12, 0x21,
            0x31, 0x41, 0x06, 0x13, 0x51, 0x61, 0x07, 0x22, 0x71, 0x14, 0x32, 0x81,
            0x91, 0xa1, 0x08, 0x23, 0x42, 0xb1, 0xc1, 0x15, 0x52, 0xd1, 0xf0, 0x24,
            0x33, 0x62, 0x72, 0x82, 0x09, 0x0a, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x25,
            0x26, 0x27, 0x28, 0x29, 0x2a, 0x34, 0x35, 0x36, 0x37, 0x38, 0x39, 0x3a,
            0x43, 0x44, 0x45, 0x46, 0x47, 0x48, 0x49, 0x4a, 0x53, 0x54, 0x55, 0x56,
            0x57, 0x58, 0x59, 0x5a, 0x63, 0x64, 0x65, 0x66, 0x67, 0x68, 0x69, 0x6a,
            0x73, 0x74, 0x75, 0x76, 0x77, 0x78, 0x79, 0x7a, 0x83, 0x84, 0x85, 0x86,
            0x87, 0x88, 0x89, 0x8a, 0x92, 0x93, 0x94, 0x95, 0x96, 0x97, 0x98, 0x99,
            0x9a, 0xa2, 0xa3, 0xa4, 0xa5, 0xa6, 0xa7, 0xa8, 0xa9, 0xaa, 0xb2, 0xb3,
            0xb4, 0xb5, 0xb6, 0xb7, 0xb8, 0xb9, 0xba, 0xc2, 0xc3, 0xc4, 0xc5, 0xc6,
            0xc7, 0xc8, 0xc9, 0xca, 0xd2, 0xd3, 0xd4, 0xd5, 0xd6, 0xd7, 0xd8, 0xd9,
            0xda, 0xe1, 0xe2, 0xe3, 0xe4, 0xe5, 0xe6, 0xe7, 0xe8, 0xe9, 0xea, 0xf1,
            0xf2, 0xf3, 0xf4, 0xf5, 0xf6, 0xf7, 0xf8, 0xf9, 0xfa, 0xff, 0xc4, 0x00,
            0x1f, 0x01, 0x00, 0x03, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01,
            0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x02, 0x03, 0x04, 0x05,
            0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0xff, 0xc4, 0x00, 0xb5, 0x11, 0x00,
            0x02, 0x01, 0x02, 0x04, 0x04, 0x03, 0x04, 0x07, 0x05, 0x04, 0x04, 0x00,
            0x01, 0x02, 0x77, 0x00, 0x01, 0x02, 0x03, 0x11, 0x04, 0x05, 0x21, 0x31,
            0x06, 0x12, 0x41, 0x51, 0x07, 0x61, 0x71, 0x13, 0x22, 0x32, 0x81, 0x08,
            0x14, 0x42, 0x91, 0xa1, 0xb1, 0xc1, 0x09, 0x23, 0x33, 0x52, 0xf0, 0x15,
            0x62, 0x72, 0xd1, 0x0a, 0x16, 0x24, 0x34, 0xe1, 0x25, 0xf1, 0x17, 0x18,
            0x19, 0x1a, 0x26, 0x27, 0x28, 0x29, 0x2a, 0x35, 0x36, 0x37, 0x38, 0x39,
            0x3a, 0x43, 0x44, 0x45, 0x46, 0x47, 0x48, 0x49, 0x4a, 0x53, 0x54, 0x55,
            0x56, 0x57, 0x58, 0x59, 0x5a, 0x63, 0x64, 0x65, 0x66, 0x67, 0x68, 0x69,
            0x6a, 0x73, 0x74, 0x75, 0x76, 0x77, 0x78, 0x79, 0x7a, 0x82, 0x83, 0x84,
            0x85, 0x86, 0x87, 0x88, 0x89, 0x8a, 0x92, 0x93, 0x94, 0x95, 0x96, 0x97,
            0x98, 0x99, 0x9a, 0xa2, 0xa3, 0xa4, 0xa5, 0xa6, 0xa7, 0xa8, 0xa9, 0xaa,
            0xb2, 0xb3, 0xb4, 0xb5, 0xb6, 0xb7, 0xb8, 0xb9, 0xba, 0xc2, 0xc3, 0xc4,
            0xc5, 0xc6, 0xc7, 0xc8, 0xc9, 0xca, 0xd2, 0xd3, 0xd4, 0xd5, 0xd6, 0xd7,
            0xd8, 0xd9, 0xda, 0xe2, 0xe3, 0xe4, 0xe5, 0xe6, 0xe7, 0xe8, 0xe9, 0xea,
            0xf2, 0xf3, 0xf4, 0xf5, 0xf6, 0xf7, 0xf8, 0xf9, 0xfa, 0xff, 0xda, 0x00,
            0x0c, 0x03, 0x01, 0x00, 0x02, 0x11, 0x03, 0x11, 0x00, 0x3f, 0x00, 0xc7,
            0xa2, 0x8a, 0x2b, 0xf4, 0xc3, 0xf3, 0xd3, 0xff, 0xd9
        ];
    }
}

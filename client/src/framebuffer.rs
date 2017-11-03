use ::{VIEW_PIXEL_FORMAT};
use protocol::rfb;
use std::ptr;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FbSize {
    pub width: usize, 
    pub height: usize
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
    pub fn no_of_bytes(&self) -> usize {
        self.no_of_pixels() * VIEW_PIXEL_FORMAT.bytes_per_pixel
    }
    pub fn stride(&self) -> usize {
        VIEW_PIXEL_FORMAT.bytes_per_pixel * self.width
    }
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

pub trait FbSlice {
    fn size(&self) -> FbSize;
    fn bytes(&mut self) -> &mut [u8];

    fn byte_pos(&self, x : usize, y : usize) -> usize {
        y * self.size().stride() + x * VIEW_PIXEL_FORMAT.bytes_per_pixel
    }

    fn set_pixel(&mut self, x : usize, y : usize,
                 r : u8, g : u8, b : u8) {
        let pos = self.byte_pos(x, y);
        let data = self.bytes();
        data[pos] = r;
        data[pos + 1] = g;
        data[pos + 2] = b;
    }

    fn set_line(&mut self, x : usize, width : usize, y : usize, line : &[Rgb]) {
        assert!(x + width <= self.size().width);
        assert!(y <= self.size().height);
        let pos = self.byte_pos(x, y);
        let data = self.bytes();
        unsafe {
            let data = data.as_mut_ptr().offset(pos as isize) as *mut Rgb;
            ptr::copy_nonoverlapping(line.as_ptr(), data, width);
        }
    }
}

pub struct Framebuffer {
    data : Vec<u8>,
    size : FbSize
}
impl Framebuffer {
    pub fn new() -> Self {
        Self {
            data: Vec::new(),
            size: FbSize::new(0, 0)
        }
    }

    pub fn data(&self) -> &Vec<u8> {
        &self.data
    }

    pub fn resize(&mut self, new_size : FbSize) {
        //TODO pixel data is not correctly transferred in this way (you must crop right-most columns and
        //bottom-most rows)
        self.size = new_size;
        let new_len = new_size.no_of_bytes();
        let gray = 0xe0u8;
        self.data.resize(new_len, gray);
    }
    
    pub fn get_raw_parts(&self) -> FbRawParts {
        FbRawParts(self.data.as_ptr(), self.size)
    }
}
impl FbSlice for Framebuffer {
    fn size(&self) -> FbSize {
        self.size
    }
    fn bytes(&mut self) -> &mut [u8] {
        &mut self.data[..]
    }
}

pub struct FbRawParts(pub *const u8, pub FbSize);
unsafe impl Send for FbRawParts { }
pub struct FbPointerSlice<'a> {
    data : &'a mut [u8],
    size : FbSize
}
impl<'a> FbPointerSlice<'a> {
    pub unsafe fn from_raw_parts(parts : FbRawParts) -> Self {
        Self {
            data: ::std::slice::from_raw_parts_mut(
                      parts.0 as *mut u8, parts.1.no_of_bytes()),
            size: parts.1
        }
    }
}
impl<'a> FbSlice for FbPointerSlice<'a> {
    fn size(&self) -> FbSize {
        self.size
    }
    fn bytes(&mut self) -> &mut [u8] {
        self.data
    }
}

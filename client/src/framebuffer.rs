use ::{FB_PIXEL_FORMAT};
use protocol::rfb;
use std::ptr;

pub enum PixelFormat {
    NativeBgrx,
    Rgb
}
impl PixelFormat {
    pub fn bytes_per_pixel(&self) -> usize {
        match *self {
            PixelFormat::NativeBgrx => 4,
            PixelFormat::Rgb => 3
        }
    }
}

pub enum FbAccess {
    Decoding,
    Resizing,
    Reading
}
impl From<FbAccess> for u32 {
    fn from(access : FbAccess) -> u32 {
        match access {
            FbAccess::Decoding => 0,
            FbAccess::Resizing => 1,
            FbAccess::Reading => 2
        }
    }
}

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
        self.no_of_pixels() * FB_PIXEL_FORMAT.bytes_per_pixel
    }
    pub fn stride(&self) -> usize {
        FB_PIXEL_FORMAT.bytes_per_pixel * self.width
    }
}

#[repr(C)]
pub struct Bgrx {
    pub b : u8,
    pub g : u8,
    pub r : u8,
    pub x : u8
}
impl Bgrx {
    pub fn from_tpixel(tpixel : rfb::TPixel) -> Self {
        Self {
            r: tpixel.r,
            g: tpixel.g,
            b: tpixel.b,
            x: 0
        }
    }
}

pub trait FbSlice {
    fn size(&self) -> FbSize;
    fn bytes(&mut self) -> &mut [u8];

    fn byte_pos(&self, x : usize, y : usize) -> usize {
        y * self.size().stride() + x * FB_PIXEL_FORMAT.bytes_per_pixel
    }

    fn set_pixel(&mut self, x : usize, y : usize,
                 r : u8, g : u8, b : u8) {
        let pos = self.byte_pos(x, y);
        let data = self.bytes();
        data[pos] = b;
        data[pos + 1] = g;
        data[pos + 2] = r;
    }

    fn set_line(&mut self, x : usize, width : usize, y : usize,
                line : &[Bgrx])
    {
        assert!(x + width <= self.size().width);
        assert!(y <= self.size().height);
        let pos = self.byte_pos(x, y);
        let data = self.bytes();
        unsafe {
            let data = data.as_mut_ptr().offset(pos as isize) as *mut Bgrx;
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
    pub unsafe fn uninitialized(size : FbSize) -> Self {
        let len = size.no_of_bytes();
        let mut data = Vec::with_capacity(len);
        data.set_len(len);
        Self {
            data: data,
            size: size
        }
    }

    pub fn data(&self) -> &Vec<u8> {
        &self.data
    }

    pub fn take_data(self) -> Vec<u8> {
        self.data
    }

    pub fn resize(&mut self, new_size : FbSize) {
        //TODO pixel data is not correctly transferred in this way (you must crop right-most columns and
        //bottom-most rows)
        self.size = new_size;
        let new_len = new_size.no_of_bytes();
        let gray = 0xe0u8;
        self.data.resize(new_len, gray);
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

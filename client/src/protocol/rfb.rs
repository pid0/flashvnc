use protocol::parsing::primitive::{u8p,pred,utf8_with_len,dep,length,prefix_len_array,u32_be,utf8,non_zero,literal,u8_bool,u16_be,ignored,i32_be,nothing};

pub const PROTOCOL_VERSION_LEN : usize = 12;

//TODO decide: either:
//  [string : pred(utf8_with_len(12), |s| s == "RFB 003.008\n", "should be...") => String]
//or:
//  [string : [utf8_with_len(12) if |s| == "RFB 003.008\n" else "should be"] -> String]

pub const SEC_TYPE_NONE : u8 = 1;
//TODO check
pub const SEC_TYPE_VNC : u8 = 2;
pub const SEC_TYPE_TIGHT : u8 = 16;

const SEC_RESULT_OK : u32 = 0;
const SEC_RESULT_FAILED : u32 = 1;

pub const ENCODING_RAW : i32 = 0;
pub const ENCODING_TIGHT : i32 = 7;
pub const ENCODING_DESKTOP_SIZE : i32 = -223;

fn is_security_type(&number : &u8) -> bool {
    number == SEC_TYPE_NONE
        || number == SEC_TYPE_VNC
        || number == SEC_TYPE_TIGHT
}

packet! { ProtocolVersion:
    [string : [pred(
        utf8_with_len(12),
        |s| s == "RFB 003.008\n",
        "should be RFB version 3.8")] -> String]
}

packet! { ErrorReason:
    [string : [dep(length(u32_be()), utf8())] -> String]
}

packet! { SecurityTypesArray:
    [types : [prefix_len_array(
        non_zero(u8p()), 
        pred(u8p(), is_security_type, "should be security type"))] -> Vec<u8>]
}
meta_packet! { SecurityTypes:
    SecurityTypesArray,
    [u8p() => 0] ErrorReason
}
packet! { SecurityResponse:
    [sec_type : [pred(u8p(), 
                      is_security_type, 
                      "should be security type")] -> u8]
}

packet! { SecurityResultOk:
    [ignored : [literal(u32_be(), SEC_RESULT_OK)] -> ()]
}
meta_packet! { SecurityResult:
    Ok(SecurityResultOk),
    [u32_be() => SEC_RESULT_FAILED] Failed(ErrorReason)
}

packet! { ClientInit:
    [shared : [u8_bool()] -> bool]
}

packet! { PixelFormat:
	[bits_per_pixel : [u8p()] -> u8]
    [depth : [u8p()] -> u8]

    [big_endian : [u8_bool()] -> bool]
    [true_color : [u8_bool()] -> bool]

    [red_max : [u16_be()] -> u16]
    [green_max : [u16_be()] -> u16]
    [blue_max : [u16_be()] -> u16]

    [red_shift : [u8p()] -> u8]
    [green_shift : [u8p()] -> u8]
    [blue_shift : [u8p()] -> u8]

    [ignored : [ignored(3)] -> ()]
}

packet! { ServerInit:
    [width : [u16_be()] -> u16]
    [height : [u16_be()] -> u16]
    [pixel_format : [PixelFormat::parser()] -> PixelFormat]
        //TODO refactor: local parser rfb_string
    [name : [dep(length(u32_be()), utf8())] -> String]
}

packet! { SetPixelFormat:
    [ignored : [ignored(3)] -> ()]
    [format : [PixelFormat::parser()] -> PixelFormat]
}

packet! { SetEncodings:
    [ignored : [ignored(1)] -> ()]
    [encodings : [prefix_len_array(u16_be(), i32_be())] -> Vec<i32>]
}

packet! { FramebufferUpdateRequest:
    [incremental : [u8_bool()] -> bool]
    [x : [u16_be()] -> u16]
    [y : [u16_be()] -> u16]
    [width : [u16_be()] -> u16]
    [height : [u16_be()] -> u16]
}

packet! { PointerEvent:
    [mask : [u8p()] -> u8]
    [x : [u16_be()] -> u16]
    [y : [u16_be()] -> u16]
}

tagged_meta_packet! { ClientToServer: u8p() => u8 =>
    [0] SetPixelFormat,
    [2] SetEncodings,
    [3] FramebufferUpdateRequest,
    [5] PointerEvent
}

packet! { RawRectangle:
    [ignored : [nothing()] -> ()] //read bytes yourself, force client somehow?
    //[bytes : [bytes(???)] -> Vec<u8>] //next: ???????
}
packet! { TightRectangle:
    [ignored : [nothing()] -> ()]
}
packet! { DesktopSizeRectangle:
    [ignored : [nothing()] -> ()]
}
//TODO solution to everything: step-by-step parsing(lazy parser)
//packet! { TrleTile(cpixel_len : usize, width : usize, height : usize):
//    u8p() => {
//        [0] { bytes : [bytes(width * height * cpixel_len)] -> Vec<u8> }
//        [1] { color : [cpixel(cpixel_len)] -> Pixel },
//        [x = 2..16] { palette : [array_with_len(2, cpixel(cpixel_len))] -> Vec<Pixel>, 
//            pixels : [bytes(ceil(width / match x { 2 => 8, x if x <= 4 => 4, _ => 8 }))] }, //need to check correctness of bit fields yourself
//        [127] { pixels : like above },
//        [128] { //repeated until the tile is done???? }
//    }
//}
//packet! { TrleRectangle:
//    [tiles]
//}

tagged_meta_packet! { RectanglePayload: i32_be() => i32 =>
    [ENCODING_RAW] RawRectangle,
    [ENCODING_TIGHT] TightRectangle,
    [ENCODING_DESKTOP_SIZE] DesktopSizeRectangle
}

packet! { Rectangle:
    //[r : [one_way_dep(RectangleHeader::parser, |r| r.width * r.height. RectanglePayload::parser())]
    [x : [u16_be()] -> u16]
    [y : [u16_be()] -> u16]
    [width : [u16_be()] -> u16] //for DesktopSize: x, y ignored; width, height fb width and height
    [height : [u16_be()] -> u16]
    [payload : [RectanglePayload::parser()] -> RectanglePayload]
}

packet! { FramebufferUpdate:
    [ignored : [ignored(1)] -> ()]
    [no_of_rectangles : [u16_be()] -> u16]
}

tagged_meta_packet! { ServerToClient: u8p() => u8 =>
    [0] FramebufferUpdate
}

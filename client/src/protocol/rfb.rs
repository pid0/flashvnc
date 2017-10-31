use protocol::parsing::primitive::{u8p,seq,conv,pred,utf8_with_len,dep,length,prefix_len_array,u32_be,utf8,non_zero,literal,u8_bool,u16_be,ignored,i32_be,nothing,zero_len};
use protocol::parsing::{Parser,Input,Output};
use protocol::parsing::result::{ParseResult,WriteResult,WriteError};

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

pub const ENCODING_WORST_JPEG_QUALITY : i32 = -512;
pub const ENCODING_BEST_JPEG_QUALITY : i32 = -412;
pub const ENCODING_COMPRESSION_LEVEL_0 : i32 = -256;
pub const ENCODING_CHROMA_SUBSAMPLING_1X : i32 = -768;
pub const ENCODING_CHROMA_SUBSAMPLING_2X : i32 = -766;
pub const ENCODING_CHROMA_SUBSAMPLING_4X : i32 = -767;

pub const ENCODING_DESKTOP_SIZE : i32 = -223;
pub const ENCODING_CURSOR : i32 = -239;
pub const ENCODING_EXTENDED_DESKTOP_SIZE : i32 = -308;

pub const ENCODING_LAST_RECT : i32 = -224;
pub const ENCODING_CONTINUOUS_UPDATES : i32 = -313;
pub const ENCODING_FENCE : i32 = -312;

pub const EXTENDED_DESKTOP_NO_ERROR : usize = 0;

pub const FENCE_BLOCK_BEFORE : u32 = 1;
pub const FENCE_BLOCK_AFTER : u32 = 2;
pub const FENCE_SYNC_NEXT : u32 = 4;
pub const FENCE_REQUEST : u32 = 0x80000000;

fn is_security_type(&number : &u8) -> bool {
    number == SEC_TYPE_NONE
        || number == SEC_TYPE_VNC
        || number == SEC_TYPE_TIGHT
}

//test
struct CompactLength;
impl Parser for CompactLength {
    type T = usize;
    fn parse<'a, I>(&self, mut input : I) -> ParseResult<usize, I>
        where I : Input<'a>
    {
        let mut ret = 0;
        for i in 0..3 {
            let (byte, rest) = input.read(1)?;
            input = rest;
            let byte = byte[0];

            let significant_bits = if i == 2 {
                byte
            } else {
                byte & 0x7f
            };
            ret |= (significant_bits as usize) << (i * 7);
            if byte & 0x80 == 0 {
                return Ok((ret, input));
            }
        }
        Ok((ret, input))
    }
    fn write<O>(&self, output : &mut O, value : usize) -> WriteResult
        where O : Output
    {
        if value > 4194303 {
            Err(WriteError::PredicateFailed("number too large"))
        } else if value > 16383 {
            output.write(
                &[(value & 0x7f) as u8 | 0x80, 
                ((value >> 7) & 0x7f) as u8 | 0x80,
                (value >> 14) as u8])
        } else if value > 127 {
            output.write(&[(value & 0x7f) as u8 | 0x80, (value >> 7) as u8])
        } else {
            output.write(&[value as u8])
        }
    }
}
pub fn compact_length() -> impl Parser<T = usize> {
    CompactLength { }
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
    [width : [length(u16_be())] -> usize]
    [height : [length(u16_be())] -> usize]
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
    [x : [length(u16_be())] -> usize]
    [y : [length(u16_be())] -> usize]
    [width : [length(u16_be())] -> usize]
    [height : [length(u16_be())] -> usize]
}

packet! { KeyEvent:
    [down : [u8_bool()] -> bool]
    [ignored : [ignored(2)] -> ()]
    [key : [u32_be()] -> u32]
}

packet! { PointerEvent:
    [mask : [u8p()] -> u8]
    [x : [u16_be()] -> u16]
    [y : [u16_be()] -> u16]
}

packet! { EnableContinuousUpdates:
    [enable : [u8_bool()] -> bool]
    [x : [length(u16_be())] -> usize]
    [y : [length(u16_be())] -> usize]
    [width : [length(u16_be())] -> usize]
    [height : [length(u16_be())] -> usize]
}

packet! { Fence:
    [ignored : [ignored(3)] -> ()]
    [flags : [u32_be()] -> u32]
        //TODO predicate len <= 64
    [payload : [prefix_len_array(u8p(), u8p())] -> Vec<u8>]
}

packet! { SetDesktopSize:
    [ignored : [ignored(1)] -> ()]
    [width : [length(u16_be())] -> usize]
    [height : [length(u16_be())] -> usize]
    [screens : [prefix_len_array(
        conv(seq(u8p(), ignored(1)), |(n, ())| n, |n| Ok((n, ()))),
        Screen::parser())] -> Vec<Screen>]
}

tagged_meta_packet! { ClientToServer: u8p() => u8 =>
    [0] SetPixelFormat,
    [2] SetEncodings,
    [3] FramebufferUpdateRequest,
    [4] KeyEvent,
    [5] PointerEvent,
    [150] EnableContinuousUpdates,
    [248] Fence,
    [251] SetDesktopSize
}

packet! { TPixel:
    [r : [u8p()] -> u8]
    [g : [u8p()] -> u8]
    [b : [u8p()] -> u8]
}

packet! { RawRectangle:
    [ignored : [nothing()] -> ()] //read bytes yourself, force client somehow?
}
packet! { TightFill:
    [control_byte : [pred(u8p(), |n| n & 0xf0 == 0b1000_0000, 
                          "bits 7..4 must be 1000")] -> u8]
}
packet! { TightJpeg:
    [control_byte : [pred(u8p(), |n| n & 0xf0 == 0b1001_0000, 
                          "bits 7..4 must be 1001")] -> u8]
    [length : [compact_length()] -> usize]
}
packet! { TightZlib:
    //TODO note that, if the uncompressed length is less than 12, there is no compression!
    [length : [compact_length()] -> usize]
}
packet! { CopyFilter:
    [ignored : [nothing()] -> ()]
}
packet! { PaletteFilter:
    [no_of_colors : [conv(u8p(), |n| (n as usize) + 1, 
                          |n| Ok((n - 1) as u8))] -> usize]
}
packet! { GradientFilter:
    [ignored : [nothing()] -> ()]
}
tagged_meta_packet! { TightFilter: u8p() => u8 =>
    [0] CopyFilter,
    [1] PaletteFilter,
    [2] GradientFilter
}
packet! { TightBasicFilterId:
    [control_byte : [pred(u8p(), |n| n & 0xc0 == 0x40, 
                          "msb must be zero, bit 6 must be 1")] -> u8]
    [filter : [TightFilter::parser()] -> TightFilter]
}
packet! { TightBasicNoFilterId:
    [control_byte : [pred(u8p(), |n| n & 0xc0 == 0, 
                          "msb must be zero, bit 6 must be 0")] -> u8]
}
meta_packet! { TightMethod:
    Jpeg(TightJpeg),
    Fill(TightFill),
    Basic(TightBasicFilterId),
    BasicNoFilterId(TightBasicNoFilterId)
}
packet! { TightRectangle:
    [control_byte : [zero_len(u8p())] -> u8]
    [method : [TightMethod::parser()] -> TightMethod]
}
packet! { DesktopSizeRectangle:
    [ignored : [nothing()] -> ()]
}
packet! { CursorRectangle:
    [ignored : [nothing()] -> ()]
}

packet! { Screen:
    [id : [u32_be()] -> u32]
    [x : [u16_be()] -> u16]
    [y : [u16_be()] -> u16]
    [width : [length(u16_be())] -> usize]
    [height : [length(u16_be())] -> usize]
    [flags : [u32_be()] -> u32]
}
packet! { ExtendedDesktopSizeRectangle:
    [screens : [prefix_len_array(
        conv(seq(u8p(), ignored(3)), |(n, ())| n, |n| Ok((n, ()))),
        Screen::parser())] -> Vec<Screen>]
}
packet! { LastRectangle:
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
    [ENCODING_DESKTOP_SIZE] DesktopSizeRectangle,
    [ENCODING_CURSOR] CursorRectangle,
    [ENCODING_EXTENDED_DESKTOP_SIZE] ExtendedDesktopSizeRectangle,
    [ENCODING_LAST_RECT] LastRectangle
}

packet! { Rectangle:
    //[r : [one_way_dep(RectangleHeader::parser, |r| r.width * r.height. RectanglePayload::parser())]
    [x : [length(u16_be())] -> usize]
    [y : [length(u16_be())] -> usize]
    [width : [length(u16_be())] -> usize]
    [height : [length(u16_be())] -> usize]
    [payload : [RectanglePayload::parser()] -> RectanglePayload]
}

packet! { FramebufferUpdate:
    [ignored : [ignored(1)] -> ()]
    [no_of_rectangles : [u16_be()] -> u16]
}

packet! { Bell:
    [ignored : [nothing()] -> ()]
}

packet! { ServerCutText:
    [ignored : [ignored(3)] -> ()]
    [string : [dep(length(u32_be()), utf8())] -> String]
}

packet! { EndOfContinuousUpdates:
    [ignored : [nothing()] -> ()]
}

tagged_meta_packet! { ServerToClient: u8p() => u8 =>
    [0] FramebufferUpdate,
    [2] Bell,
    [3] ServerCutText,
    [150] EndOfContinuousUpdates,
    [248] Fence
}

#[cfg(test)]
mod the_compact_length_parser {
    use super::*;
    use protocol::parsing::parser_test::*;
    use protocol::parsing::result::WriteError;

    fn should_be_able_to_parse_from_itself(n : usize) {
        let output = write(&compact_length(), n).unwrap();
        assert_eq!(parse(&compact_length(), &output[..]).unwrap(), n);
    }

    #[test]
    fn should_encode_numbers_that_fit_into_7_bits_with_1_byte() {
        assert_eq!(parse(&compact_length(), &[0x01][..]).unwrap(), 1);
        assert_eq!(write(&compact_length(), 1).unwrap(), [0x01]);

        assert_eq!(parse(&compact_length(), &[127][..]).unwrap(), 127);
        assert_eq!(write(&compact_length(), 127).unwrap(), [127]);
    }

    #[test]
    fn should_encode_larger_numbers_by_prepending_bytes_starting_with_a_1() {
        assert_eq!(write(&compact_length(), 255).unwrap(), [0xff, 0x01]);
        should_be_able_to_parse_from_itself(255);
        assert_eq!(write(&compact_length(), 256).unwrap(), [0x80, 0x02]);
        should_be_able_to_parse_from_itself(256);

        assert_eq!(write(&compact_length(), 10_000).unwrap(), [0x90, 0x4e]);
        should_be_able_to_parse_from_itself(10_000);

        assert_eq!(write(&compact_length(), 16384).unwrap(), 
                   [0x80, 0x80, 0x01]);
        should_be_able_to_parse_from_itself(16384);
    }

    #[test]
    fn should_use_3_bytes_max_and_the_third_one_in_full() {
        assert_eq!(write(&compact_length(), 4194303).unwrap(), 
                   [0xff, 0xff, 0xff]);
        should_be_able_to_parse_from_itself(4194303);

        match write(&compact_length(), 4194304).unwrap_err() {
            WriteError::PredicateFailed(error) => {
                assert_eq!(error, "number too large");
            },
            _ => {
                assert!(false);
            }
        }
    }
}

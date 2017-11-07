#[macro_use]
pub mod parsing;

pub mod rfb;

use self::parsing::primitive::{literal,u8p,i16_be};

packet! { VirtualMouseServerMessage:
    [ignored : [literal(u8p(), 0)] -> ()]
    [button_mask : [u8p()] -> u8]
    [dx : [i16_be()] -> i16]
    [dy : [i16_be()] -> i16]
}

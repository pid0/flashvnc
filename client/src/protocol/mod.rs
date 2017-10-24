#[macro_use]
pub mod parsing;

pub mod rfb;

use self::parsing::primitive::{literal,u8p,i8p};

packet! { VirtualMouseServerMessage:
    [ignored : [literal(u8p(), 0)] -> ()]
    [button_mask : [u8p()] -> u8]
    [dx : [i8p()] -> i8]
    [dy : [i8p()] -> i8]
}

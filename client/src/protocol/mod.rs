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

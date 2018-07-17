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

use std::io;
use std::cell::RefCell;
use protocol::parsing::{Parser,io_parse,io_write};
use protocol::parsing::result::{ParseEndResult,WriteError};

pub fn parse<P, I>(parser : &P, input : I) -> ParseEndResult<P::T>
    where P : Parser,
          I : io::Read
{
    let buffer = RefCell::new(Vec::new());
    io_parse(parser, &buffer, input)
}

pub fn write<P>(parser : &P, value : P::T) -> Result<Vec<u8>, WriteError>
    where P : Parser
{
    let mut ret = Vec::new();
    io_write(parser, &mut ret, value)?;
    Ok(ret)
}

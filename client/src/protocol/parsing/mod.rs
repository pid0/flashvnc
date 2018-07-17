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
pub mod result;
#[macro_use]
pub mod packet;
pub mod primitive;
pub mod io_input;
#[cfg(test)]
pub mod parser_test;

use std::io;
use std::cell::{RefCell,Ref};

use self::io_input::{IoInput,SharedBuf};
use self::result::{ParseEndResult,ParseResult,WriteResult};

pub trait Input<'a> : Clone + ::std::fmt::Debug {
    fn read(&mut self, len : usize) -> ParseResult<Ref<'a, [u8]>, Self>;
}

pub trait Output {
    fn write(&mut self, bytes : &[u8]) -> WriteResult;
}
//TODO move to own file
pub struct IoOutput<T>
    where T : io::Write
{
    writer : T
}
impl<T> IoOutput<T>
    where T : io::Write
{
    pub fn new(writer : T) -> Self {
        IoOutput {
            writer: writer
        }
    }
}
impl<T> Output for IoOutput<T>
    where T : io::Write
{
    fn write(&mut self, bytes : &[u8]) -> WriteResult {
        self.writer.write_all(bytes)?;
        Ok(())
    }
}

trait Writable {
    fn write<O>(&self, output : &mut O) -> WriteResult
        where O : Output;
}
impl Writable for u8 {
    fn write<O>(&self, output : &mut O) -> WriteResult
        where O : Output
    {
        output.write(&[*self])
    }
}
impl Writable for u16 {
    fn write<O>(&self, output : &mut O) -> WriteResult
        where O : Output
    {
        output.write(&[((self & 0xff00) >> 8) as u8, (self & 0xff) as u8])
    }
}

//TODO use pub use for IoInput and stuff
pub trait Parser {
    type T : Clone;

    fn parse<'a, I>(&self, input : I) -> ParseResult<Self::T, I>
        where I : Input<'a>;
    fn write<O>(&self, output : &mut O, value : Self::T) -> WriteResult
        where O : Output;
}

pub trait Packet : Sized {
    fn parse<I>(buffer : &SharedBuf, input : I) -> ParseEndResult<Self>
        where I : io::Read;
    fn write<O>(self, output : O) -> WriteResult
        where O : io::Write;
    fn name() -> &'static str;
}

pub fn io_parse<P, I>(parser : &P, buffer : &SharedBuf, input : I) 
    -> ParseEndResult<P::T>
    where P : Parser,
          I : io::Read
{
    let input_ref = RefCell::new(input);
    unsafe {
        buffer.borrow_mut().set_len(0);
    }
    let input_pointer = IoInput::new(&buffer, &input_ref);
    match parser.parse(input_pointer) {
        Ok((ret, _)) => Ok(ret),
        Err((error, rest)) => Err((error, rest.offset()))
    }
}

pub fn io_write<P, O>(parser : &P, output : O, value : P::T) -> WriteResult
    where P : Parser,
          O : io::Write
{
    parser.write(&mut IoOutput::new(output), value)
}

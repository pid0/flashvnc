#[macro_use]
pub mod result;
#[macro_use]
pub mod packet;
pub mod primitive;
pub mod io_input;
#[cfg(test)]
mod parser_test;

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
pub struct IoOutput<'a, T>
    where T : io::Write + 'a
{
    writer : &'a mut T
}
impl<'a, T> IoOutput<'a, T>
    where T : io::Write + 'a
{
    pub fn new(writer : &'a mut T) -> Self {
        IoOutput {
            writer: writer
        }
    }
}
impl<'a, T> Output for IoOutput<'a, T>
    where T : io::Write + 'a
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
    fn write<O>(self, output : &mut O) -> WriteResult
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

pub fn io_write<P, O>(parser : &P, output : &mut O, value : P::T) -> WriteResult
    where P : Parser,
          O : io::Write
{
    parser.write(&mut IoOutput::new(output), value)
}

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

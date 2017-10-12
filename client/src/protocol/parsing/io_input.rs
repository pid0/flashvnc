use std::io;
use std::cell::{RefCell,Ref};
use protocol::parsing::Input;
use protocol::parsing::result::ParseResult;
use std::fmt;
use std::fmt::{Formatter,Debug};

pub type SharedBuf = RefCell<Vec<u8>>;

#[derive(Derivative)]
#[derivative(Clone(bound=""))]
pub struct IoInput<'a, T>
    where T : io::Read + 'a
{
    buffer : &'a SharedBuf,
    reader : &'a RefCell<T>,
    offset : usize
}
impl<'a, T> IoInput<'a, T>
    where T : io::Read
{
    pub fn new(buffer : &'a SharedBuf, reader : &'a RefCell<T>) -> Self {
        IoInput {
            buffer: buffer,
            reader: reader,
            offset: 0
        }
    }

    fn rest(&self, offset : usize) -> Self {
        IoInput {
            buffer: self.buffer,
            reader: self.reader,
            offset: self.offset + offset
        }
    }

    pub fn offset(&self) -> usize {
        self.offset
    }
}
impl<'a, T> Input<'a> for IoInput<'a, T>
    where T : io::Read
{
    fn read(&mut self, len : usize) -> ParseResult<Ref<'a, [u8]>, Self>
    {
        let buf_len = self.buffer.borrow().len();
        let available = buf_len - self.offset;
        if len > available {
            //TODO refactor into own function
            let mut buf = self.buffer.borrow_mut();
            let needed = len - available;
            //TODO subtract current capacity? -> probably not
            buf.reserve(needed);
            unsafe {
                buf.set_len(buf_len + needed);
            }
            try_parse!(self.clone(), 
                       self.reader.borrow_mut().read_exact(
                           &mut buf[self.offset + available..]));
        }
        Ok((Ref::map(self.buffer.borrow(), 
                  |buf| &buf[self.offset..(self.offset + len)]), 
            self.rest(len)))
    }
}
//TODO remove? needed in assertions
impl<'a, T> Debug for IoInput<'a, T>
    where T : io::Read
{
    fn fmt(&self, formatter : &mut Formatter) -> Result<(), fmt::Error> {
        formatter.write_fmt(format_args!("at byte position {}", self.offset))?;
        Ok(())
    }
}

#[cfg(test)]
mod the_io_input {
    use std::cell::RefCell;
    use super::*;

    macro_rules! fixture {
        ($input:ident, $bytes:expr) => {
            let buffer = RefCell::new(Vec::new());
            let bytes = $bytes;
            let bytes_ref = RefCell::new(&bytes[..]);
            let mut $input = IoInput::new(&buffer, &bytes_ref);
        }
    }

    #[test]
    fn should_read_once_and_then_return_the_first_n_bytes() {
        fixture!(input, [1u8, 2, 3]);
        assert_eq!(*input.read(1).unwrap().0, [1u8]);
        assert_eq!(*input.read(2).unwrap().0, [1u8, 2]);
        assert_eq!(*input.read(1).unwrap().0, [1u8]);
    }

    #[test]
    fn should_return_an_input_operating_on_the_rest() {
        fixture!(input, [1u8, 2, 3]);
        let mut after_first_byte = input.read(1).unwrap().1;
        assert_eq!(*after_first_byte.read(2).unwrap().0, [2u8, 3]);
    }

    #[test]
    fn should_return_an_io_error_if_one_occurs() {
        fixture!(too_short, [1u8]);
        let error = too_short.read(2).unwrap_err().0;
        let message = format!("{:?}", error);
        assert!(message.to_lowercase().contains("eof"));
    }
}

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

pub struct BitBuffer<'a> {
    data : &'a [u8],
    byte : u8,
    bit : usize,
    byte_pos : usize
}
impl<'a> BitBuffer<'a> {
    pub fn new(data : &'a [u8]) -> BitBuffer {
        Self {
            data: data,
            byte: 0,
            bit: 0,
            byte_pos: 0
        }
    }

    pub fn next(&mut self) -> u8 {
        if self.bit == 0 {
            self.byte = self.data[self.byte_pos];
            self.byte_pos += 1;
        }

        let ret = (self.byte & 0x80) >> 7;
        self.byte <<= 1;

        self.bit += 1;
        if self.bit == 8 {
            self.next_byte();
        }

        ret
    }

    pub fn next_byte(&mut self) {
        self.bit = 0;
    }
}

#[cfg(test)]
mod a_bit_buffer {
    use super::*;

    #[test]
    fn should_read_bit_from_msb_to_lsb() {
        let mut buf = BitBuffer::new(&[0xc4, 0x80][..]);
        assert_eq!(buf.next(), 1);
        assert_eq!(buf.next(), 1);
        assert_eq!(buf.next(), 0);
        assert_eq!(buf.next(), 0);

        assert_eq!(buf.next(), 0);
        assert_eq!(buf.next(), 1);

        buf.next_byte();
        assert_eq!(buf.next(), 1);
        assert_eq!(buf.next(), 0);
    }
}

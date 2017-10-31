use protocol::parsing::{Parser,Input,Output,Writable};
use protocol::parsing::result::{ParseResult,ParseError,WriteResult,WriteError};
use std::marker::PhantomData;

//TODO primitive.rs, combinator.rs

//TODO note about compromises (no use of closures, ...)

pub struct Ignored {
    len : usize
}
impl Parser for Ignored {
    type T = ();
    fn parse<'a, I>(&self, mut input : I) -> ParseResult<Self::T, I>
        where I : Input<'a>
    {
        let (_, rest) = input.read(self.len)?;
        Ok(((), rest))
    }
    //TODO where T : Writable
    fn write<O>(&self, output : &mut O, () : ()) -> WriteResult
        where O : Output
    {
        for _ in 0..self.len {
            output.write(&[0][..])?;
        }
        Ok(())
    }
}
pub fn ignored(len : usize) -> Ignored {
    Ignored {
        len: len
    }
}
pub fn nothing() -> Ignored {
    ignored(0)
}

//TODO remove
//pub struct Literal<T>
//    where T : Clone
//{
//    value : T
//}
////TODO implement
//impl<T> Parser for Literal<T>
//    where T : Clone
//{
//    type T = T;
//    fn parse<'a, I>(&self, input : I) -> ParseResult<Self::T, I>
//        where I : Input<'a>
//    {
//        Ok((self.get(), input))
//    }
//    //TODO where T : Writable
//    fn write<O>(&self, _output : &mut O, _value : Self::T) -> WriteResult
//        where O : Output
//    {
//        Ok(())
//    }
//}
//impl<T> Literal<T>
//    where T : Clone
//{
//    fn new(value : T) -> Self {
//        Literal::<T> {
//            value: value
//        }
//    }
//}
//pub fn literal<T>(value : T) -> impl Parser<T = T>
//    where T : Clone
//{
//    Literal::<T>::new(value)
//}

struct U8;
impl Parser for U8 {
    type T = u8;
    fn parse<'a, I>(&self, mut input : I) -> ParseResult<u8, I>
        where I : Input<'a>
    {
        let (bytes, rest) = input.read(1)?;
        Ok((bytes[0], rest))
    }
    fn write<O>(&self, output : &mut O, value : u8) -> WriteResult
        where O : Output
    {
        value.write(output)
    }
}
pub fn u8p() -> impl Parser<T = u8> {
    U8 { }
}
pub fn i8p() -> impl Parser<T = i8> {
    conv(u8p(), |u| u as i8, |i| Ok(i as u8))
}

//TODO refactor: write directly in write func
struct U16Be;
impl Parser for U16Be {
    type T = u16;
    fn parse<'a, I>(&self, mut input : I) -> ParseResult<u16, I>
        where I : Input<'a>
    {
        let (bytes, rest) = input.read(2)?;
        Ok((((bytes[0] as u16) << 8) | (bytes[1] as u16), rest))
    }
    fn write<O>(&self, output : &mut O, value : u16) -> WriteResult
        where O : Output
    {
        value.write(output)
    }
}
pub fn u16_be() -> impl Parser<T = u16> {
    U16Be { }
}

struct U32Be;
impl Parser for U32Be {
    type T = u32;
    fn parse<'a, I>(&self, mut input : I) -> ParseResult<u32, I>
        where I : Input<'a>
    {
        let (bytes, rest) = input.read(4)?;
        let first = bytes[0] as u32;
        let second = bytes[1] as u32;
        let third = bytes[2] as u32;
        let fourth = bytes[3] as u32;
        let value = fourth | (third << 8) | (second << 16) | (first << 24);
        Ok((value, rest))
    }
    fn write<O>(&self, output : &mut O, value : u32) -> WriteResult
        where O : Output
    {
        let byte0 = ((value & 0xff000000) >> 24) as u8;
        let byte1 = ((value & 0x00ff0000) >> 16) as u8;
        let byte2 = ((value & 0x0000ff00) >> 8) as u8;
        let byte3 = (value & 0x000000ff) as u8;
        output.write(&[byte0, byte1, byte2, byte3])
    }
}
pub fn u32_be() -> impl Parser<T = u32> {
    U32Be { }
}

struct I32Be;
impl Parser for I32Be {
    type T = i32;
    fn parse<'a, I>(&self, mut input : I) -> ParseResult<i32, I>
        where I : Input<'a>
    {
        let (bytes, rest) = input.read(4)?;
        let byte0 = bytes[0] as u32;
        let byte1 = bytes[1] as u32;
        let byte2 = bytes[2] as u32;
        let byte3 = bytes[3] as u32;
        let value = byte3 | (byte2 << 8) | (byte1 << 16) | (byte0 << 24);
        Ok((value as i32, rest))
    }
    fn write<O>(&self, output : &mut O, value : i32) -> WriteResult
        where O : Output
    {
        let value = value as u32;
        let byte0 = ((value & 0xff000000) >> 24) as u8;
        let byte1 = ((value & 0x00ff0000) >> 16) as u8;
        let byte2 = ((value & 0x0000ff00) >> 8) as u8;
        let byte3 = (value & 0x000000ff) as u8;
        output.write(&[byte0, byte1, byte2, byte3])
    }
}
pub fn i32_be() -> impl Parser<T = i32> {
    I32Be { }
}

pub struct Utf8 {
    len : Option<usize>
}
impl Parser for Utf8 {
    type T = String;
    fn parse<'a, I>(&self, input : I) -> ParseResult<String, I>
        where I : Input<'a>
    {
        self.parse_with_params(input, self.len.unwrap())
    }
    fn write<O>(&self, output : &mut O, value : String) -> WriteResult
        where O : Output
    {
        //TODO check correct length
        output.write(value.as_bytes())
    }
}
impl ParameterizedParser for Utf8 {
    type Params = usize;

    fn parse_with_params<'a, I>(&self, mut input : I, len : usize) 
        -> ParseResult<Self::T, I>
        where I : Input<'a>
    {
        let (bytes, rest) = input.read(len)?;
        //TODO improve slice -> Vec conversion somehow
        let string = try_parse!(
            input, String::from_utf8(bytes.iter().cloned().collect()));
        Ok((string, rest))
    }
    fn extract_params(&self, value : &String) -> usize {
        value.as_bytes().len()
    }
}
pub fn utf8_with_len(len : usize) -> Utf8 {
    Utf8 { 
        len: Some(len)
    }
}
pub fn utf8() -> Utf8 {
    Utf8 {
        len: None
    }
}

pub struct Seq<P1, P2>
    where P1 : Parser,
          P2 : Parser
{
    p1 : P1,
    p2 : P2
}
impl<P1, P2> Parser for Seq<P1, P2>
    where P1 : Parser,
          P2 : Parser
{
    type T = (P1::T, P2::T);
    fn parse<'a, I>(&self, input : I) -> ParseResult<Self::T, I>
        where I : Input<'a>
    {
        let (value_1, first_rest) = self.p1.parse(input)?;
        let (value_2, rest) = self.p2.parse(first_rest)?;
        Ok(((value_1, value_2), rest))
    }
    fn write<O>(&self, output : &mut O, (value_1, value_2) : Self::T) 
        -> WriteResult
        where O : Output
    {
        self.p1.write(output, value_1)?;
        self.p2.write(output, value_2)?;
        Ok(())
    }
}
pub fn seq<P1, P2>(p1 : P1, p2 : P2) -> impl Parser<T = (P1::T, P2::T)>
    where P1 : Parser,
          P2 : Parser
{
    Seq::<P1, P2> { p1: p1, p2: p2 }
}

pub struct Conv<P, F, FBack, U>
    where P : Parser,
          F : Fn(P::T) -> U,
          FBack : Fn(U) -> Result<P::T, WriteError>,
          U : Clone
{
    p : P,
    f : F,
    f_back : FBack,
    u : PhantomData<U>
}
impl<P, F, FBack, U> Parser for Conv<P, F, FBack, U>
    where P : Parser,
          F : Fn(P::T) -> U,
          FBack : Fn(U) -> Result<P::T, WriteError>,
          U : Clone
{
    type T = U;
    fn parse<'a, I>(&self, input : I) -> ParseResult<Self::T, I>
        where I : Input<'a>
    {
        let (value, rest) = self.p.parse(input)?;
        Ok(((self.f)(value), rest))
    }
    fn write<O>(&self, output : &mut O, value : Self::T) -> WriteResult
        where O : Output
    {
        self.p.write(output, (self.f_back)(value)?)
    }
}
pub fn conv<P, F, FBack, U>(p : P, f : F, f_back : FBack) -> impl Parser<T = U>
    where P : Parser,
          F : Fn(P::T) -> U,
          FBack : Fn(U) -> Result<P::T, WriteError>,
          U : Clone
{
    Conv::<P, F, FBack, U> { p: p, f: f, f_back: f_back, u: PhantomData }
}

struct Pred<P, F>
    where P : Parser,
          F : Fn(&P::T) -> bool
{
    p : P, 
    f : F,
    description : &'static str
}
impl<P, F> Parser for Pred<P, F>
    where P : Parser,
          F : Fn(&P::T) -> bool
{
    type T = P::T;
    fn parse<'a, I>(&self, input : I) -> ParseResult<Self::T, I>
        where I : Input<'a>
    {
        let (ret, rest) = self.p.parse(input.clone())?;
        match (self.f)(&ret) {
            true => Ok((ret, rest)),
            false => Err((ParseError::PredicateFailed(self.description), input))
        }
    }
    fn write<O>(&self, output : &mut O, value : Self::T) -> WriteResult
        where O : Output
    {
        //TODO check predicate
        self.p.write(output, value)
    }
}
pub fn pred<P, F>(p : P, f : F, description : &'static str) 
    -> impl Parser<T = P::T>
    where P : Parser,
          F : Fn(&P::T) -> bool
{
    Pred {
        p: p,
        f: f,
        description: description
    }
}

struct Opt<P1, P2, T>
    where P1 : Parser<T = T>,
          P2 : Parser<T = T>,
          T : Clone
{
    p1 : P1,
    p2 : P2,
    t : PhantomData<T>
}
impl<P1, P2, T> Parser for Opt<P1, P2, T>
    where P1 : Parser<T = T>,
          P2 : Parser<T = T>,
          T : Clone
{
    type T = T;
    fn parse<'a, I>(&self, input : I) -> ParseResult<Self::T, I>
        where I : Input<'a>
    {
        match self.p1.parse(input.clone()) {
            Ok(x) => Ok(x),
            Err(_) => self.p2.parse(input)
        }
    }
    fn write<O>(&self, output : &mut O, value : Self::T) -> WriteResult
        where O : Output
    {
        //TODO handle only logic errors (not IoError)?
        let clone = value.clone();
        match self.p1.write(output, value) {
            Ok(()) => Ok(()),
            Err(_) => self.p2.write(output, clone)
        }
    }
}
pub fn opt<P1, P2, T>(p1 : P1, p2 : P2) -> impl Parser<T = T>
    where P1 : Parser<T = T>,
          P2 : Parser<T = T>,
          T : Clone
{
    Opt {
        p1: p1,
        p2: p2,
        t: PhantomData
    }
}

//TODO move
pub trait Number : Copy + Eq + Ord + From<u8> {
    //TODO read/write
}
//impl<T : Copy + PartialEq> Number for T { }
impl Number for u8 {
}
impl Number for u16 {
}
impl Number for u32 {
}

pub fn is_constant<P, T>(p : P, constant : T) -> impl Parser<T = T>
    where P : Parser<T = T>,
          T : Number
{
    pred(p, move |&x| x == constant, "should be a constant number")
}

pub fn const_prefix<PrefixParser, T, P>(
    prefix : PrefixParser, constant : T, suffix : P)
    -> impl Parser<T = P::T>
    where PrefixParser : Parser<T = T>,
          T : Number,
          P : Parser
{
    conv(
        seq(is_constant(prefix, constant), suffix),
        |pair| pair.1,
        move |alone| Ok((constant, alone)))
}

pub fn literal<P, T>(p : P, constant : T) -> impl Parser<T = ()>
    where P : Parser<T = T>,
          T : Number
{
    const_prefix(p, constant, nothing())
}

pub fn non_zero<P, T>(p : P) -> impl Parser<T = T>
    where P : Parser<T = T>,
          T : Number
{
    pred(p, |&x| x > From::from(0), "should be non-zero")
}

pub fn u8_bool() -> impl Parser<T = bool> {
    conv(u8p(),
        |number| number > 0,
        |boolean| Ok(if boolean {
            1
        } else {
            0
        }))
}

//TODO move to dep from here --------->

pub trait ParameterizedParser : Parser {
    type Params : Copy;

    fn parse_with_params<'a, I>(&self, input : I, params : Self::Params)
        -> ParseResult<Self::T, I>
        where I : Input<'a>;
    fn extract_params(&self, value : &Self::T) -> Self::Params;
}

pub trait Length : Copy + Clone {
    fn from_usize(l : usize) -> Self;
    fn to_usize(self) -> usize;
}
impl Length for u8 {
    fn from_usize(l : usize) -> Self { l as u8 }
    fn to_usize(self) -> usize { self as usize }
}
impl Length for u16 {
    fn from_usize(l : usize) -> Self { l as u16 }
    fn to_usize(self) -> usize { self as usize }
}
impl Length for u32 {
    fn from_usize(l : usize) -> Self { l as u32 }
    fn to_usize(self) -> usize { self as usize }
}

pub struct Array<P>
    where P : Parser
{
    p : P
}
impl<P> Parser for Array<P>
    where P : Parser
{
    type T = Vec<P::T>;
    fn parse<'a, I>(&self, _input : I) -> ParseResult<Self::T, I>
        where I : Input<'a>
    {
        unimplemented!()
    }
    fn write<O>(&self, output : &mut O, vec : Self::T) -> WriteResult
        where O : Output
    {
        for item in vec {
            self.p.write(output, item)?;
        }
        Ok(())
    }
}
impl<P> ParameterizedParser for Array<P>
    where P : Parser
{
    type Params = usize;

    fn parse_with_params<'a, I>(&self, mut input : I, length : usize)
        -> ParseResult<Self::T, I>
        where I : Input<'a>
    {
        let mut ret = Vec::with_capacity(length);
        for _ in 0..length {
            let (item, rest) = self.p.parse(input)?;
            ret.push(item);
            input = rest;
        }
        Ok((ret, input))
    }
    fn extract_params(&self, value : &Self::T) -> usize {
        value.len()
    }
}

pub fn array<P>(p : P) -> Array<P>
    where P : Parser
{
    Array { p: p }
}

//pub struct LazyVec<T> {
//    len : usize
//}
//impl<T> LazyVec<T> {
//    fn parse<'a, I>(&self, _input : I) -> ParseResult<T, I>
//        where I : Input<'a>
//    {
//        unimplemented!()
//    }
//}
//pub struct LazyArray<P>
//    where P : Parser
//{
//    p : P
//}
//impl<P> Parser for Array<P>
//    where P : Parser
//{
//    type T = Vec<P::T>;
//    fn parse<'a, I>(&self, _input : I) -> ParseResult<Self::T, I>
//        where I : Input<'a>
//    {
//        unimplemented!()
//    }
//    fn write<O>(&self, output : &mut O, vec : Self::T) -> WriteResult
//        where O : Output
//    {
//        for item in vec {
//            self.p.write(output, item)?;
//        }
//        Ok(())
//    }
//}
//impl<P> ParameterizedParser for Array<P>
//    where P : Parser
//{
//    type Params = usize;
//
//    fn parse_with_params<'a, I>(&self, mut input : I, length : usize)
//        -> ParseResult<Self::T, I>
//        where I : Input<'a>
//    {
//        let mut ret = Vec::with_capacity(length);
//        for _ in 0..length {
//            let (item, rest) = self.p.parse(input)?;
//            ret.push(item);
//            input = rest;
//        }
//        Ok((ret, input))
//    }
//    fn extract_params(&self, value : &Self::T) -> usize {
//        value.len()
//    }
//}
//
//pub fn array<P>(p : P) -> Array<P>
//    where P : Parser
//{
//    Array { p: p }
//}

struct Dep<P1, P2>
    where P1 : Parser<T = P2::Params>,
          P2 : ParameterizedParser
{
    p1 : P1,
    p2 : P2
}
impl<P1, P2> Parser for Dep<P1, P2>
    where P1 : Parser<T = P2::Params>,
          P2 : ParameterizedParser
{
    type T = P2::T;
    fn parse<'a, I>(&self, input : I) -> ParseResult<Self::T, I>
        where I : Input<'a>
    {
        let (params, after_prefix) = self.p1.parse(input)?;
        self.p2.parse_with_params(after_prefix, params)
    }
    fn write<O>(&self, output : &mut O, value : Self::T) -> WriteResult
        where O : Output
    {
        self.p1.write(output, self.p2.extract_params(&value))?;
        self.p2.write(output, value)?;
        Ok(())
    }
}
pub fn dep<P1, P2>(p1 : P1, p2 : P2) -> impl Parser<T = P2::T>
    where P1 : Parser<T = P2::Params>,
          P2 : ParameterizedParser
{
    Dep { p1: p1, p2: p2 }
}

pub fn length<P, T>(p : P) -> impl Parser<T = usize>
    where P : Parser<T = T>,
          T : Length
{
    //TODO error if number too large
    conv(p, |t| t.to_usize(), |size| Ok(T::from_usize(size)))
}

pub fn prefix_len_array<LengthParser, T, ItemParser>(
    length_parser : LengthParser, 
    item_parser : ItemParser) -> impl Parser<T = Vec<ItemParser::T>>
    where LengthParser : Parser<T = T>,
          T : Length,
          ItemParser : Parser
{
    dep(length(length_parser), array(item_parser))
}

// <----------- until here
struct ZeroLen<P>
    where P : Parser
{
    p : P
}
impl<P> Parser for ZeroLen<P>
    where P : Parser
{
    type T = P::T;
    fn parse<'a, I>(&self, input : I) -> ParseResult<Self::T, I>
        where I : Input<'a>
    {
        let original_input = input.clone();
        let (ret, _) = self.p.parse(input)?;
        Ok((ret, original_input))
    }
    fn write<O>(&self, _output : &mut O, _value : Self::T) -> WriteResult
        where O : Output
    {
        Ok(())
    }
}
pub fn zero_len<P>(p : P) -> impl Parser<T = P::T>
    where P : Parser
{
    ZeroLen { p: p }
}

#[cfg(test)]
mod the_u8_parser {
    use super::u8p;
    use protocol::parsing::parser_test::parse;

    #[test]
    fn should_get_a_single_byte_from_the_input() {
        let input = [4u8];
        let parser = u8p();
        assert_eq!(parse(&parser, &input[..]).unwrap(), 4u8);
    }
}

#[cfg(test)]
mod the_ignored_parser {
    use super::*;
    use protocol::parsing::parser_test::*;

    #[test]
    fn should_write_the_specified_number_of_bytes_as_zeroes() {
        assert_eq!(write(&ignored(2), ()).unwrap(), [0, 0]);
    }

    #[test]
    fn should_eat_up_the_specified_amount_of_bytes_when_parsing() {
        let parser = seq(ignored(2), u8p());
        assert_eq!(parse(&parser, &[1, 2, 3][..]).unwrap(), ((), 3));
    }
}

#[cfg(test)]
mod the_u32_be_parser {
    use super::*;
    use protocol::parsing::parser_test::*;
    use std::u32;

    #[test]
    fn should_read_and_write_four_bytes_in_big_endian() {
        let input = [1u8, 2, 3, 4];
        assert_eq!(parse(&u32_be(), &input[..]).unwrap(), 16909060u32);
        assert_eq!(write(&u32_be(), u32::MAX).unwrap(), 
                   [0xff, 0xff, 0xff, 0xff]);
    }
}

#[cfg(test)]
mod the_i32_be_parser {
    use super::*;
    use protocol::parsing::parser_test::*;

    #[test]
    fn should_represent_numbers_as_four_byte_twos_complement() {
        assert_eq!(parse(&i32_be(), &[0x40, 0x01, 0x00, 0x03][..]).unwrap(),
            2i32.pow(30) + 2i32.pow(16) + 3i32);
        assert_eq!(parse(&i32_be(), &[0xff, 0xff, 0xff, 0xff][..]).unwrap(),
            -1);
        assert_eq!(parse(&i32_be(), &[0x80, 0x80, 0x80, 0x80][..]).unwrap(),
            -2139062144);
        assert_eq!(write(&i32_be(), -2).unwrap(), [0xff, 0xff, 0xff, 0xfe]);
    }
}

#[cfg(test)]
mod the_seq_parser {
    use super::*;
    use protocol::parsing::parser_test::*;
    //TODO refactor away
    use protocol::parsing::ParseResult;

    #[test]
    fn should_call_the_second_parser_with_the_rest_from_the_first() {
        struct Second;
        impl Parser for Second {
            type T = ();
            fn parse<'a, I>(&self, mut input : I) -> ParseResult<Self::T, I>
                where I : Input<'a>
            {
                assert_eq!(*input.read(2).unwrap().0, [2u8, 3]);
                Ok(((), input))
            }
            fn write<O>(&self, _output : &mut O, _value : Self::T) -> WriteResult
                where O : Output {
                Ok(())
            }
        }

        let input = [1u8, 2, 3];
        let parser = seq(u8p(), Second { });
        assert_eq!(parse(&parser, &input[..]).unwrap(), (1u8, ()));
    }

    #[test]
    fn should_write_the_first_value_then_the_second() {
        let parser = seq(u8p(), u16_be());
        assert_eq!(write(&parser, (1, 2)).unwrap(), [1u8, 0, 2]);
    }
}

#[cfg(test)]
mod the_conv_parser {
    use super::*;
    use protocol::parsing::parser_test::write;

    #[test]
    fn should_convert_the_value_back_before_writing() {
        let parser = conv(u8p(), |x| 4 * x, |x| Ok(x / 2));
        assert_eq!(write(&parser, 8).unwrap(), [4]);
    }

    #[test]
    fn should_forward_a_back_conversion_error() {
        let parser = conv(u8p(), |x| x, 
                          |_| Err(WriteError::ConversionFailed("")));
        match write(&parser, 1).unwrap_err() {
            WriteError::ConversionFailed(_) => { },
            _ => assert!(false)
        }
    }
}

#[cfg(test)]
mod the_pred_parser {
    use super::*;
    use protocol::parsing::parser_test::parse;

    #[test]
    fn should_return_the_original_input_if_the_predicate_fails() {
        let input = [1u8];
        let parser = pred(u8p(), |_| false, "");
        let result = parse(&parser, &input[..]);
        assert_eq!(result.unwrap_err().1, 0);
    }
}

#[cfg(test)]
mod the_dep_parser {
    use super::*;
    use protocol::parsing::parser_test::parse;

    #[test]
    fn should_call_the_second_parser_with_params_from_the_first() {
        struct Second;
        impl Parser for Second {
            type T = bool;
            fn parse<'a, I>(&self, _input : I) -> ParseResult<Self::T, I>
                where I : Input<'a>
            {
                unimplemented!()
            }
            fn write<O>(&self, _output : &mut O, _value : Self::T)
                -> WriteResult
                where O : Output
            {
                unimplemented!()
            }
        }
        impl ParameterizedParser for Second {
            type Params = u8;
            fn parse_with_params<'a, I>(&self, input : I, param : u8)
                -> ParseResult<Self::T, I>
                where I : Input<'a>
            {
                assert_eq!(param, 12);
                Ok((true, input))
            }
            fn extract_params(&self, _value : &Self::T) -> u8 {
                unimplemented!()
            }
        }

        let input = [12u8, 2, 3];
        let parser = dep(u8p(), Second);
        assert!(parse(&parser, &input[..]).unwrap());
    }
}

#[cfg(test)]
mod dep_with_array_parser {
    use super::*;
    use protocol::parsing::parser_test::*;

    #[test]
    fn should_write_an_array_with_prefixed_length_the_same_way_it_was_read() {
        let input = [3u8, 1, 2, 3];
        let parser = dep(length(u8p()), array(u8p()));
        let array = parse(&parser, &input[..]).unwrap();
        assert_eq!(write(&parser, array).unwrap(), [3, 1, 2, 3]);
    }
}

#[cfg(test)]
mod the_literal_parser {
    use super::*;
    use protocol::parsing::parser_test::*;

    #[test]
    fn should_only_succeed_when_a_constant_is_parsed_but_return_nothing() {
        let parser = literal(u32_be(), 5);
        assert_eq!(parse(&parser, &[0, 0, 0, 5][..]).unwrap(), ());
        parse(&parser, &[0, 0, 0, 4][..]).unwrap_err();
    }

    #[test]
    fn should_write_the_specified_constant_out_of_no_input() {
        let parser = literal(u16_be(), 1);
        assert_eq!(write(&parser, ()).unwrap(), [0, 1]);
    }
}

#[cfg(test)]
mod the_zero_len_parser {
    use super::*;
    use protocol::parsing::parser_test::*;

    #[test]
    fn should_return_the_unadvanced_input() {
        let parser = seq(zero_len(u8p()), u8p());
        assert_eq!(parse(&parser, &[10][..]).unwrap(), (10, 10));
    }

    #[test]
    fn should_write_nothing() {
        assert_eq!(write(&zero_len(u8p()), 5).unwrap(), []);
    }
}

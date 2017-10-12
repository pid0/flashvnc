//TODO merge all macros into one (@parser, @nested_sequence prefix)
macro_rules! parser {
    ( $e:expr ) => {
        $e
    };
}

macro_rules! nested_sequence {
    ( $first:expr ) => {
        $first
    };
    ( $first:expr, $($rest:expr),* ) => {
        ::protocol::parsing::primitive::seq(
            $first, nested_sequence!($($rest),*))
    }
}

macro_rules! nested_pattern {
    ( @field ignored ) => {
        ()
    };
    ( @field $field:ident ) => {
        $field
    };
    ( $first:ident ) => {
        nested_pattern!(@field $first)
    };
    ( $first:ident, $($rest:ident),* ) => {
        (nested_pattern!(@field $first), nested_pattern!($($rest),*))
    }
}

//TODO e -> parser
macro_rules! object_to_tuple {
    ( @access $obj:expr => ignored ) => {
        ()
    };
    ( @access $obj:expr => $field:ident ) => {
        $obj.$field
    };
    ( $obj:expr => $first:ident ) => {
        object_to_tuple!(@access $obj => $first)
    };
    ( $obj:expr => $first:ident, $($rest:ident),+ ) => {
        (object_to_tuple!(@access $obj => $first), 
         object_to_tuple!($obj => $($rest),+))
    }
}

macro_rules! packet_impl {
    ( $name:ident ) => {
        impl ::protocol::parsing::Packet for $name {
            fn parse<I>(
                buffer : &::protocol::parsing::io_input::SharedBuf,
                input : I) -> ::protocol::parsing::result::ParseEndResult<Self>
                where I : ::std::io::Read
            {
                ::protocol::parsing::io_parse(&Self::parser(), buffer, input)
            }

            fn write<O>(self, output : &mut O)
                -> ::protocol::parsing::result::WriteResult
                where O : ::std::io::Write
            {
                ::protocol::parsing::io_write(&Self::parser(), output, self)
            }

            fn name() -> &'static str {
                stringify!($name)
            }
        }
    }
}

#[macro_export]
macro_rules! packet {
    ( $name:ident: 
      $( 
          [ $field:ident : [$($e:tt)*] -> $t:ty ] 
      )+ ) => 
    {
        packet! { @filter_ignored_fields $name:
            [$($field : $t,)+]
            []
            $([$field : [$($e)*] -> $t])+
        }
    };
    ( @filtered $name:ident: 
      $(
          @[ $stored_field:ident : $stored_t:ty ]
      )*
      $( 
          [ $field:ident : [$($e:tt)*] -> $t:ty ] 
      )+ ) => 
    {
        #[derive(Debug, Clone)]
        pub struct $name {
            $(
                pub $stored_field : $stored_t
            ),*
        }
        impl $name {
            pub fn parser() -> impl ::protocol::parsing::Parser<T = Self> {
                ::protocol::parsing::primitive::conv(
                    nested_sequence!($(parser!($($e)*)),*),
                    |tuple| {
                        let nested_pattern!($($field),*) = tuple;
                        Self {
                            $(
                                $stored_field : $stored_field
                            ),*
                        }
                    },
                    |_object| Ok(object_to_tuple!(_object => $($field),+))
                )
            }
        }
        packet_impl! {
            $name
        }
    };

    ( @filter_ignored_fields $name:ident: 
      [ignored : $first_t:ty,]
      [$($result:ident : $result_t:ty,)*]
      $( 
          [ $field:ident : [$($e:tt)*] -> $t:ty ] 
      )+ ) => {
        packet! { @filtered $name:
            $(@[$result : $result_t])*
            $([$field : [$($e)*] -> $t])+
        }
    };
    ( @filter_ignored_fields $name:ident: 
      [$first:ident : $first_t:ty,]
      [$($result:ident : $result_t:ty,)*]
      $( 
          [ $field:ident : [$($e:tt)*] -> $t:ty ] 
      )+ ) => {
        packet! { @filtered $name:
            $(@[$result : $result_t])*
            @[$first : $first_t]
            $([$field : [$($e)*] -> $t])+
        }
    };
    ( @filter_ignored_fields $name:ident: 
      [ignored : $first_t:ty, $($input:ident : $input_t:ty,)*]
      [$($result:ident : $result_t:ty,)*]
      $( 
          [ $field:ident : [$($e:tt)*] -> $t:ty ] 
      )+ ) => {
        packet! { @filter_ignored_fields $name:
            [$($input : $input_t,)*]
            [$($result : $result_t,)*]
            $([$field : [$($e)*] -> $t])+
        }
    };
    ( @filter_ignored_fields $name:ident: 
      [$first:ident : $first_t:ty, $($input:ident : $input_t:ty,)*]
      [$($result:ident : $result_t:ty,)*]
      $( 
          [ $field:ident : [$($e:tt)*] -> $t:ty ] 
      )+ ) => {
        packet! { @filter_ignored_fields $name:
            [$($input : $input_t,)*]
            [$($result : $result_t,)* $first : $first_t,]
            $([$field : [$($e)*] -> $t])+
        }
    };

}

//TODO refactor (with nested_sequence)!
#[macro_export]
macro_rules! nested_opt {
    ( $first:expr ) => {
        $first
    };
    ( $first:expr, $($rest:expr),* ) => {
        ::protocol::parsing::primitive::opt(
            $first, nested_opt!($($rest),*))
    }
}

#[macro_export]
macro_rules! opt_parser {
    ( [$prefix_parser:expr => $prefix_value:expr] $e:expr ) => {
        ::protocol::parsing::primitive::const_prefix(
            $prefix_parser, $prefix_value, $e)
    };
    ( $e:expr ) => {
        $e
    }
}

#[macro_export]
macro_rules! meta_packet {
    ( 
        $name:ident: $(
            $([$($prefix:tt)*])* $sub_packet:ident
        ),+ 
    ) => {
        meta_packet! { $name:
            $(
                $([$($prefix)*])* $sub_packet($sub_packet)
            ),+
        }
    };
    ( 
        $name:ident: $(
            $([$($prefix:tt)*])* $variant:ident($sub_packet:ident)
        ),+ 
    ) => {
        #[derive(Debug, Clone)]
        pub enum $name {
            $(
                $variant($sub_packet),
            )*
        }
        impl $name {
            pub fn parser() -> impl ::protocol::parsing::Parser<T = Self> {
                nested_opt! {
                    $(
                        opt_parser!(
                            $([$($prefix)*])*
                            ::protocol::parsing::primitive::conv(
                                $sub_packet::parser(),
                                |raw| $name::$variant(raw),
                                #[allow(unreachable_patterns)]
                                |wrapped| match wrapped {
                                    $name::$variant(inner) => Ok(inner),
                                    _ => Err(::protocol::parsing::result
                                             ::WriteError::ConversionFailed(
                                                 concat!("must be variant ",
                                                 stringify!($variant))))
                                })
                        )
                    ),+
                }
            }

            pub fn write<O>(self, output : &mut O)
                -> ::protocol::parsing::result::WriteResult
                where O : ::std::io::Write
            {
                ::protocol::parsing::io_write(&Self::parser(), output, self)
            }
        }
        packet_impl! {
            $name
        }
    }
}

//TODO remove
//macro_rules! discriminator_to_index {
//    ( $first:expr, => $index:expr => $($result:tt)* ) => {
//        |d| match d {
//            $($result)*
//            $first => $index
//        }
//    };
//    ( $first:expr, $($rest:expr,)+ => $index:expr => $($result:tt)* ) => {
//        discriminator_to_index!(
//            $($rest,)+ => 
//            $index + 1
//            $($result)*
//            $first => $index
//        )
//    }
//}

#[macro_export]
macro_rules! tagged_meta_packet {
    (
        $name:ident: $discriminator_parser:expr => $discriminator_type:ty =>
        $(
            [$discriminator:tt] $sub_packet:ident
        ),+
    ) => {
        #[derive(Debug, Clone)]
        pub enum $name {
            $(
                $sub_packet($sub_packet)
            ),+
        }
        impl $name {
            pub fn parser() -> impl ::protocol::parsing::Parser<T = Self> {
                struct Parser<P>
                    where P : ::protocol::parsing::Parser<
                        T = $discriminator_type>
                {
                    discriminator_parser : P
                }
                impl<P> ::protocol::parsing::Parser for Parser<P>
                    where P : ::protocol::parsing::Parser<
                        T = $discriminator_type>
                {
                    type T = $name;
                    fn parse<'a, I>(&self, input : I)
                        -> ::protocol::parsing::result::ParseResult<Self::T, I>
                        where I : ::protocol::parsing::Input<'a>
                    {
                        //TODO eventually, use parser() here and below, not new_parser()
                        let (discriminator, after_prefix) = 
                            self.discriminator_parser.parse(input.clone())?;
                        match discriminator {
                            $(
                                $discriminator => {
                                    let (ret, rest) = $sub_packet::parser()
                                        .parse(after_prefix)?;
                                    Ok(($name::$sub_packet(ret), rest))
                                },
                            )+
                            d => Err((::protocol::parsing::result::ParseError
                                ::InvalidDiscriminator(d as u64), input))
                        }
                    }
                    fn write<O>(&self, output : &mut O, value : Self::T)
                        -> ::protocol::parsing::result::WriteResult
                        where O : ::protocol::parsing::Output
                    {
                        match value {
                            $(
                                $name::$sub_packet(x) => {
                                    self.discriminator_parser.write(
                                        output, $discriminator)?;
                                    $sub_packet::parser().write(output, x)?;
                                }
                            )+
                        }
                        Ok(())
                    }
                }
                Parser {
                    discriminator_parser: $discriminator_parser
                }
            }
        }
        packet_impl! {
            $name
        }
    }
}

#[cfg(test)]
mod the_packet_macro {
    use protocol::parsing::primitive::{ignored,u8p};
    use protocol::parsing::parser_test::{parse,write};

    #[test]
    #[allow(dead_code)]
    fn should_define_a_struct_with_the_specified_fields() {
        packet! { Packet:
            [first : [ignored(1)] -> ()]
            [second : [u8p()] -> u8]
        }
        let _p = Packet {
            first: (),
            second: 5u8
        };
    }

    #[test]
    #[allow(dead_code)]
    fn should_construct_a_parser_by_sequencing_the_subparsers() {
        packet! { PixelFormat:
            [bits_per_pixel : [u8p()] -> u8]
            [depth : [u8p()] -> u8]
        }

        let input = [32u8, 8u8];
        let format = parse(&PixelFormat::parser(), &input[..]).unwrap();
        assert_eq!(format.bits_per_pixel, 32);
        assert_eq!(format.depth, 8);
    }

    #[test]
    fn should_allow_ignoring_certain_fields_with_unity_type() {
        packet! { Foo:
            [ignored : [ignored(2)] -> ()]
            [x : [u8p()] -> u8]
            [ignored : [ignored(1)] -> ()]
        }
        assert_eq!(write(&Foo::parser(), Foo {
            x: 5
        }).unwrap(), [0, 0, 5, 0]);
    }
}

#[cfg(test)]
#[allow(dead_code)]
mod the_meta_packet_macro {
    use protocol::parsing::primitive::{u8p,pred};
    use protocol::parsing::parser_test::parse;

    packet! { A:
        [x : [pred(u8p(), |&x| x == 0, "")] -> u8]
    }
    packet! { B:
        [y : [pred(u8p(), |&x| x == 1, "")] -> u8]
    }
    packet! { C:
        [z : [pred(u8p(), |&x| x == 2, "")] -> u8]
    }

    #[test]
    fn should_define_an_enum_with_other_packets_as_variants() {
        meta_packet! { M:
            A, B
        }

        let _a = M::A(A { x: 0 });
        let _b = M::B(B { y: 0 });
    }

    #[test]
    fn should_construct_an_opt_parser_out_of_the_sub_packet_parsers() {
        meta_packet! { M:
            A, B, C
        }

        match parse(&M::parser(), &[0][..]).unwrap() {
            M::A(a) => assert_eq!(a.x, 0),
            _ => assert!(false)
        }
        match parse(&M::parser(), &[1][..]).unwrap() {
            M::B(_) => { },
            _ => assert!(false)
        }
        match parse(&M::parser(), &[2][..]).unwrap() {
            M::C(_) => { },
            _ => assert!(false)
        }
    }

    #[test]
    fn should_support_adding_constant_prefixes_to_existing_parsers() {
        meta_packet! { M:
            A,
            [u8p() => 5] B
        }

        match parse(&M::parser(), &[5, 1][..]).unwrap() {
            M::B(b) => assert_eq!(b.y, 1),
            _ => assert!(false)
        }
        parse(&M::parser(), &[4, 1][..]).unwrap_err();
    }

    #[test]
    fn should_implement_writing_out_a_variant_of_the_enum() {
        meta_packet! { M:
            A, [u8p() => 24] B, C
        }
        let variant = M::B(B { y: 1 });

        let mut output = Vec::new();
        variant.write(&mut output).unwrap();

        assert_eq!(&output[..], &[24u8, 1u8]);
    }

    #[test]
    fn should_allow_renaming_all_variants() {
        meta_packet! { M:
            Foo(A)
        }
        match parse(&M::parser(), &[0][..]).unwrap() {
            M::Foo(a) => assert_eq!(a.x, 0)
        }
    }
}

#[cfg(test)]
//#[allow(dead_code)]
//TODO here
mod the_tagged_meta_packet_macro {
    use protocol::parsing::primitive::{u16_be,u8p};
    use protocol::parsing::result::ParseError;
    use protocol::parsing::parser_test::*;

    packet! { A:
        [x : [u8p()] -> u8]
    }
    type B = A;

    tagged_meta_packet! { M: u16_be() => u16 =>
        [0] A,
        [24] B
    }

    #[test]
    fn should_define_an_enum_with_a_discriminator_for_the_variants() {
        match parse(&M::parser(), &[0, 0, 5][..]).unwrap() {
            M::A(a) => assert_eq!(a.x, 5),
            _ => assert!(false)
        }
        match parse(&M::parser(), &[0, 24, 3][..]).unwrap() {
            M::B(a) => assert_eq!(a.x, 3),
            _ => assert!(false)
        }
    }

    #[test]
    fn should_have_its_parser_fail_upon_getting_an_unhandled_discriminator() {
        match parse(&M::parser(), &[0, 1][..]).unwrap_err().0 {
            ParseError::InvalidDiscriminator(d) => assert_eq!(d, 1),
            _ => assert!(false)
        }
    }

    #[test]
    fn should_have_its_parser_write_the_discriminator() {
        assert_eq!(write(&M::parser(), M::B(B { x: 105 })).unwrap(), 
                   [0, 24, 105])
    }
}

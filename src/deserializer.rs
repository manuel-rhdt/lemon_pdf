use serde::{de, Deserializer};

use crate::serializer::error::Error;

use nom::bytes::complete::tag;
use nom::character::complete::digit1;
use nom::combinator::map_res;
use nom::combinator::{opt, recognize};
use nom::error::ErrorKind;
use nom::sequence::{pair, tuple};
use nom::IResult;
use nom::InputTakeAtPosition;

#[derive(Copy, Clone, Debug)]
enum IndirectReferenceDe {
    Both(i64, i64),
    One(i64),
}

#[derive(Debug)]
enum Token<'bytes> {
    Name(&'bytes [u8]),
    StartDict,
    EndDict,
    StartArray,
    EndArray,
    IndirectReference(i64, i64),
    PartialIndirectReference(i64),
}

fn is_space(chr: u8) -> bool {
    match chr {
        0 | 9 | 10 | 12 | 13 | 32 => true,
        _ => false,
    }
}

fn multispace0(input: &[u8]) -> IResult<&[u8], &[u8]> {
    input.split_at_position_complete(|item| !is_space(item))
}

fn multispace1(input: &[u8]) -> IResult<&[u8], &[u8]> {
    input.split_at_position1_complete(|item| !is_space(item), ErrorKind::MultiSpace)
}

fn int_num(i: &[u8]) -> IResult<&[u8], i64> {
    map_res(recognize(pair(opt(tag(b"-")), digit1)), |s: &[u8]| {
        std::str::from_utf8(s).unwrap().parse::<i64>()
    })(i)
}

fn parse_indirect_reference(i: &[u8]) -> IResult<&[u8], (i64, i64)> {
    let (i, (num, _, gen, ..)) = tuple((
        int_num,
        multispace1,
        int_num,
        multispace1,
        tag(b"R"),
        multispace1,
    ))(i)?;
    Ok((i, (num, gen)))
}

#[derive(Debug)]
enum State {
    Both(i64, i64),
    One(i64),
    None,
}

#[derive(Debug)]
pub struct PdfDeserializer<'bytes> {
    pub input: &'bytes [u8],
    state: Option<State>,
}

impl<'bytes> PdfDeserializer<'bytes> {
    fn take(&mut self, n: usize) -> &'bytes [u8] {
        let result = &self.input[..n];
        self.input = &self.input[n..];
        result
    }

    fn take_ws(&mut self) -> &'bytes [u8] {
        let mut result: &[u8] = &[];
        for sublice in self.input.split(|&chr| !chr.is_ascii_whitespace()) {
            result = sublice;
            break;
        }
        self.take(result.len());
        result
    }

    fn take_until_ws(&mut self) -> &'bytes [u8] {
        let mut result: &[u8] = &[];
        for sublice in self.input.split(|&chr| chr.is_ascii_whitespace()) {
            result = sublice;
            break;
        }
        self.take(result.len());
        result
    }

    fn next_token(&mut self) -> Result<Token, Error> {
        self.take_ws();

        if self.input.starts_with(b"/") {
            self.take(1);
            Ok(Token::Name(self.take_until_ws()))
        } else if self.input.starts_with(b"<<") {
            self.take(2);
            Ok(Token::StartDict)
        } else if self.input.starts_with(b">>") {
            self.take(2);
            Ok(Token::EndDict)
        } else if self.input.starts_with(b"[") {
            self.take(1);
            Ok(Token::StartArray)
        } else if self.input.starts_with(b"]") {
            self.take(1);
            Ok(Token::EndArray)
        } else if let Ok((input, (num, gen))) = parse_indirect_reference(self.input) {
            self.input = input;
            Ok(Token::IndirectReference(num, gen))
        } else {
            panic!()
        }
    }
}

impl<'de> Deserializer<'de> for &mut PdfDeserializer<'de> {
    type Error = Error;
    fn deserialize_any<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        match self.state.take() {
            Some(State::Both(num, gen)) => {
                self.state = Some(State::One(gen));
                visitor.visit_i64(num)
            }
            Some(State::One(gen)) => {
                self.state = Some(State::None);
                visitor.visit_i64(gen)
            }
            Some(State::None) => visitor.visit_str(&"R"),
            None => match self.next_token()? {
                Token::Name(text) => visitor.visit_str(std::str::from_utf8(text)?),
                Token::IndirectReference(num, gen) => {
                    self.state = Some(State::Both(num, gen));
                    visitor.visit_seq(self)
                }
                _ => todo!(),
            },
        }
    }

    serde::forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string
        bytes byte_buf option unit unit_struct newtype_struct seq tuple
        tuple_struct map struct enum identifier ignored_any
    }

    // fn deserialize_bool<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    // where
    //     V: de::Visitor<'de>,
    // {
    //     todo!()
    // }
    // fn deserialize_i8<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    // where
    //     V: de::Visitor<'de>,
    // {
    //     todo!()
    // }
    // fn deserialize_i16<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    // where
    //     V: de::Visitor<'de>,
    // {
    //     todo!()
    // }
    // fn deserialize_i32<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    // where
    //     V: de::Visitor<'de>,
    // {
    //     todo!()
    // }
    // fn deserialize_i64<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    // where
    //     V: de::Visitor<'de>,
    // {
    //     todo!()
    // }
    // fn deserialize_u8<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    // where
    //     V: de::Visitor<'de>,
    // {
    //     todo!()
    // }
    // fn deserialize_u16<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    // where
    //     V: de::Visitor<'de>,
    // {
    //     todo!()
    // }
    // fn deserialize_u32<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    // where
    //     V: de::Visitor<'de>,
    // {
    //     todo!()
    // }
    // fn deserialize_u64<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    // where
    //     V: de::Visitor<'de>,
    // {
    //     todo!()
    // }
    // fn deserialize_f32<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    // where
    //     V: de::Visitor<'de>,
    // {
    //     todo!()
    // }
    // fn deserialize_f64<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    // where
    //     V: de::Visitor<'de>,
    // {
    //     todo!()
    // }
    // fn deserialize_char<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    // where
    //     V: de::Visitor<'de>,
    // {
    //     todo!()
    // }
    // fn deserialize_str<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    // where
    //     V: de::Visitor<'de>,
    // {
    //     todo!()
    // }
    // fn deserialize_string<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    // where
    //     V: de::Visitor<'de>,
    // {
    //     todo!()
    // }
    // fn deserialize_bytes<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    // where
    //     V: de::Visitor<'de>,
    // {
    //     todo!()
    // }
    // fn deserialize_byte_buf<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    // where
    //     V: de::Visitor<'de>,
    // {
    //     todo!()
    // }
    // fn deserialize_option<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    // where
    //     V: de::Visitor<'de>,
    // {
    //     todo!()
    // }
    // fn deserialize_unit<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    // where
    //     V: de::Visitor<'de>,
    // {
    //     todo!()
    // }
    // fn deserialize_unit_struct<V>(
    //     self,
    //     name: &'static str,
    //     visitor: V,
    // ) -> Result<V::Value, Self::Error>
    // where
    //     V: de::Visitor<'de>,
    // {
    //     todo!()
    // }
    // fn deserialize_newtype_struct<V>(
    //     self,
    //     name: &'static str,
    //     visitor: V,
    // ) -> Result<V::Value, Self::Error>
    // where
    //     V: de::Visitor<'de>,
    // {
    //     todo!()
    // }
    // fn deserialize_seq<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    // where
    //     V: de::Visitor<'de>,
    // {
    //     todo!()
    // }
    // fn deserialize_tuple<V>(self, len: usize, visitor: V) -> Result<V::Value, Self::Error>
    // where
    //     V: de::Visitor<'de>,
    // {
    //     todo!()
    // }
    // fn deserialize_tuple_struct<V>(
    //     self,
    //     name: &'static str,
    //     len: usize,
    //     visitor: V,
    // ) -> Result<V::Value, Self::Error>
    // where
    //     V: de::Visitor<'de>,
    // {
    //     todo!()
    // }
    // fn deserialize_map<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    // where
    //     V: de::Visitor<'de>,
    // {
    //     todo!()
    // }
    // fn deserialize_struct<V>(
    //     self,
    //     name: &'static str,
    //     fields: &'static [&'static str],
    //     visitor: V,
    // ) -> Result<V::Value, Self::Error>
    // where
    //     V: de::Visitor<'de>,
    // {
    //     todo!()
    // }
    // fn deserialize_enum<V>(
    //     self,
    //     name: &'static str,
    //     variants: &'static [&'static str],
    //     visitor: V,
    // ) -> Result<V::Value, Self::Error>
    // where
    //     V: de::Visitor<'de>,
    // {
    //     todo!()
    // }
    // fn deserialize_identifier<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    // where
    //     V: de::Visitor<'de>,
    // {
    //     todo!()
    // }
    // fn deserialize_ignored_any<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    // where
    //     V: de::Visitor<'de>,
    // {
    //     todo!()
    // }
}

impl<'de> de::MapAccess<'de> for &mut PdfDeserializer<'de> {
    type Error = Error;
    fn next_key_seed<K>(&mut self, seed: K) -> Result<Option<K::Value>, Self::Error>
    where
        K: de::DeserializeSeed<'de>,
    {
        match seed.deserialize(&mut **self) {
            Ok(val) => Ok(Some(val)),
            Err(Error::SpuriousDictEnd) => Ok(None),
            Err(other_error) => Err(other_error),
        }
    }
    fn next_value_seed<V>(&mut self, seed: V) -> Result<V::Value, Self::Error>
    where
        V: de::DeserializeSeed<'de>,
    {
        seed.deserialize(&mut **self)
    }
}

impl<'de> de::SeqAccess<'de> for &mut PdfDeserializer<'de> {
    type Error = Error;

    fn next_element_seed<T>(&mut self, seed: T) -> Result<Option<T::Value>, Self::Error>
    where
        T: de::DeserializeSeed<'de>,
    {
        if self.state.is_none() {
            return Ok(None);
        }
        Ok(Some(seed.deserialize(&mut **self)?))
    }
}

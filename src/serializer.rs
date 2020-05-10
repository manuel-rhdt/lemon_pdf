use serde::{ser, Serializer};

pub mod error;

use error::{Error, Result};

use std::io::Write;

#[derive(Debug)]
pub struct PdfSerializer<W> {
    pub output: W,
}

impl<'b, W: Write> Serializer for &'b mut PdfSerializer<W> {
    type Ok = ();
    type Error = Error;

    type SerializeSeq = Self;
    type SerializeTuple = Self;
    type SerializeTupleStruct = TupleStructSerializer<'b, W>;
    type SerializeTupleVariant = TupleStructSerializer<'b, W>;
    type SerializeMap = Self;
    type SerializeStruct = Self;
    type SerializeStructVariant = Self;

    fn serialize_bool(self, v: bool) -> Result<Self::Ok> {
        match v {
            true => write!(self.output, "true")?,
            false => write!(self.output, "false")?,
        };
        Ok(())
    }
    fn serialize_i8(self, v: i8) -> Result<Self::Ok> {
        Ok(write!(self.output, "{}", v)?)
    }
    fn serialize_i16(self, v: i16) -> Result<Self::Ok> {
        Ok(write!(self.output, "{}", v)?)
    }
    fn serialize_i32(self, v: i32) -> Result<Self::Ok> {
        Ok(write!(self.output, "{}", v)?)
    }
    fn serialize_i64(self, v: i64) -> Result<Self::Ok> {
        Ok(write!(self.output, "{}", v)?)
    }
    fn serialize_u8(self, v: u8) -> Result<Self::Ok> {
        Ok(write!(self.output, "{}", v)?)
    }
    fn serialize_u16(self, v: u16) -> Result<Self::Ok> {
        Ok(write!(self.output, "{}", v)?)
    }
    fn serialize_u32(self, v: u32) -> Result<Self::Ok> {
        Ok(write!(self.output, "{}", v)?)
    }
    fn serialize_u64(self, v: u64) -> Result<Self::Ok> {
        Ok(write!(self.output, "{}", v)?)
    }
    fn serialize_f32(self, v: f32) -> Result<Self::Ok> {
        Ok(write!(self.output, "{}", v)?)
    }
    fn serialize_f64(self, v: f64) -> Result<Self::Ok> {
        Ok(write!(self.output, "{}", v)?)
    }
    fn serialize_char(self, v: char) -> Result<Self::Ok> {
        Ok(write!(self.output, "({})", v)?)
    }
    fn serialize_str(self, v: &str) -> Result<Self::Ok> {
        self.serialize_bytes(v.as_bytes())
    }
    fn serialize_bytes(self, bytes: &[u8]) -> Result<Self::Ok> {
        self.output.write_all(b"(")?;
        for &byte in bytes {
            match byte {
                0x0c /* Form Feed */ => self.output.write_all(b"\\f")?,
                0x08 /* Backspace */ => self.output.write_all(b"\\b")?,
                b'\t' => self.output.write_all(b"\\t")?,
                b'\r' => self.output.write_all(b"\\r")?,
                b'\n' => self.output.write_all(b"\\n")?,
                b')' => self.output.write_all(b"\\)")?,
                b'(' => self.output.write_all(b"\\(")?,
                non_graphic if !byte.is_ascii_graphic() => write!(self.output, "\\d{:03o}", non_graphic)?,
                other => self.output.write_all(&[other])?
            }
        }
        self.output.write_all(b")")?;
        Ok(())
    }
    fn serialize_none(self) -> Result<Self::Ok> {
        Ok(write!(self.output, "null")?)
    }
    fn serialize_some<T: ?Sized>(self, value: &T) -> Result<Self::Ok>
    where
        T: serde::Serialize,
    {
        value.serialize(self)
    }
    fn serialize_unit(self) -> Result<Self::Ok> {
        self.serialize_none()
    }
    fn serialize_unit_struct(self, name: &'static str) -> Result<Self::Ok> {
        self.serialize_str(name)
    }
    fn serialize_unit_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
    ) -> Result<Self::Ok> {
        Ok(write!(self.output, "/{}", variant)?)
    }
    fn serialize_newtype_struct<T: ?Sized>(self, _name: &'static str, value: &T) -> Result<Self::Ok>
    where
        T: serde::Serialize,
    {
        value.serialize(self)
    }
    fn serialize_newtype_variant<T: ?Sized>(
        self,
        _name: &'static str,
        _variant_index: u32,
        _variant: &'static str,
        _value: &T,
    ) -> Result<Self::Ok>
    where
        T: serde::Serialize,
    {
        todo!()
    }
    fn serialize_seq(self, _len: Option<usize>) -> Result<Self::SerializeSeq> {
        write!(self.output, "[ ")?;
        Ok(self)
    }
    fn serialize_tuple(self, len: usize) -> Result<Self::SerializeTuple> {
        self.serialize_seq(Some(len))
    }
    fn serialize_tuple_struct(
        self,
        name: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeTupleStruct> {
        Ok(TupleStructSerializer {
            serializer: self,
            name,
        })
    }
    fn serialize_tuple_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeTupleVariant> {
        Ok(TupleStructSerializer {
            serializer: self,
            name: variant,
        })
    }
    fn serialize_map(self, _len: Option<usize>) -> Result<Self::SerializeMap> {
        write!(self.output, "<< ")?;
        Ok(self)
    }
    fn serialize_struct(self, name: &'static str, _len: usize) -> Result<Self::SerializeStruct> {
        write!(self.output, "<< ")?;
        if !name.is_empty() {
            write!(self.output, "/Type /{}\n", name)?;
        }
        Ok(self)
    }
    fn serialize_struct_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        _variant: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeStructVariant> {
        todo!()
    }
    fn collect_str<T: ?Sized>(self, value: &T) -> Result<Self::Ok>
    where
        T: std::fmt::Display,
    {
        Ok(write!(self.output, "{}", value)?)
    }
}

#[derive(Debug)]
pub struct TupleStructSerializer<'a, W> {
    serializer: &'a mut PdfSerializer<W>,
    name: &'static str,
}

impl<'a, W: Write> ser::SerializeSeq for &'a mut PdfSerializer<W> {
    type Ok = ();
    type Error = Error;
    fn serialize_element<T: ?Sized>(&mut self, value: &T) -> Result<()>
    where
        T: serde::Serialize,
    {
        value.serialize(&mut **self)?;
        Ok(write!(self.output, " ")?)
    }
    fn end(self) -> Result<Self::Ok> {
        Ok(write!(self.output, "]")?)
    }
}

impl<'a, W: Write> ser::SerializeTuple for &'a mut PdfSerializer<W> {
    type Ok = ();
    type Error = Error;
    fn serialize_element<T: ?Sized>(&mut self, value: &T) -> Result<()>
    where
        T: serde::Serialize,
    {
        ser::SerializeSeq::serialize_element(self, value)
    }
    fn end(self) -> Result<Self::Ok> {
        ser::SerializeSeq::end(self)
    }
}

impl<'a, W: Write> ser::SerializeTupleStruct for TupleStructSerializer<'a, W> {
    type Ok = ();
    type Error = Error;
    fn serialize_field<T: ?Sized>(&mut self, value: &T) -> Result<()>
    where
        T: serde::Serialize,
    {
        ser::SerializeSeq::serialize_element(&mut self.serializer, value)
    }
    fn end(self) -> Result<Self::Ok> {
        Ok(write!(self.serializer.output, "{}", self.name)?)
    }
}

impl<'a, W: Write> ser::SerializeTupleVariant for TupleStructSerializer<'a, W> {
    type Ok = ();
    type Error = Error;
    fn serialize_field<T: ?Sized>(&mut self, value: &T) -> Result<()>
    where
        T: serde::Serialize,
    {
        ser::SerializeTupleStruct::serialize_field(self, value)
    }
    fn end(self) -> Result<Self::Ok> {
        ser::SerializeTupleStruct::end(self)
    }
}

impl<'a, W: Write> ser::SerializeMap for &'a mut PdfSerializer<W> {
    type Ok = ();
    type Error = Error;
    fn serialize_key<T: ?Sized>(&mut self, key: &T) -> Result<()>
    where
        T: serde::Serialize,
    {
        ser::SerializeSeq::serialize_element(self, key)
    }
    fn serialize_value<T: ?Sized>(&mut self, value: &T) -> Result<()>
    where
        T: serde::Serialize,
    {
        value.serialize(&mut **self)?;
        Ok(write!(self.output, "\n")?)
    }
    fn end(self) -> Result<Self::Ok> {
        Ok(write!(self.output, ">>")?)
    }
}

impl<'a, W: Write> ser::SerializeStruct for &'a mut PdfSerializer<W> {
    type Ok = ();
    type Error = Error;
    fn serialize_field<T: ?Sized>(&mut self, key: &'static str, value: &T) -> Result<()>
    where
        T: serde::Serialize,
    {
        self.serialize_unit_variant("struct", 0, key)?;
        write!(self.output, " ")?;
        ser::SerializeMap::serialize_value(self, value)
    }
    fn end(self) -> Result<Self::Ok> {
        Ok(write!(self.output, ">>")?)
    }
}

impl<'a, W: Write> ser::SerializeStructVariant for &'a mut PdfSerializer<W> {
    type Ok = ();
    type Error = Error;
    fn serialize_field<T: ?Sized>(&mut self, _key: &'static str, _value: &T) -> Result<()>
    where
        T: serde::Serialize,
    {
        todo!()
    }
    fn end(self) -> Result<Self::Ok> {
        todo!()
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use serde::Serialize;

    fn test_serializer<T: Serialize>(val: T, against: &'static str) {
        let mut content: Vec<u8> = vec![];
        let serializer = &mut PdfSerializer {
            output: &mut content,
        };
        val.serialize(serializer).unwrap();
        assert_eq!(std::str::from_utf8(&content).unwrap(), against)
    }

    #[test]
    fn test_sequence() {
        test_serializer(["a", "b"], "[ (a) (b) ]")
    }

    #[test]
    fn test_escape_sequence() {
        test_serializer(["\n", "\x00"], "[ (\\n) (\\d000) ]")
    }

    #[test]
    fn test_tuple_struct() {
        #[derive(Serialize)]
        #[serde(rename = "cmd")]
        struct Command(i64, &'static str);
        test_serializer(Command(10, "ten"), "10 (ten) cmd");
        test_serializer([Command(10, "ten")], "[ 10 (ten) cmd ]")
    }

    #[test]
    fn test_struct() {
        #[derive(Serialize)]
        struct TStruct {
            a: u32,
            b: &'static str,
        };
        test_serializer(
            TStruct { a: 10, b: "ten" },
            "<< /Type /TStruct\n/a 10\n/b (ten)\n>>",
        );

        #[derive(Serialize)]
        #[serde(rename = "")]
        struct TStruct2 {
            a: u32,
            b: &'static str,
        };
        test_serializer(TStruct2 { a: 10, b: "ten" }, "<< /a 10\n/b (ten)\n>>")
    }

    #[test]
    fn test_enum() {
        #[derive(Serialize)]
        enum TEnum {
            A,
            B(u32, u32),
        };

        #[derive(Serialize)]
        struct CStruct {
            a: u32,
            b: &'static str,
        }

        test_serializer(TEnum::A, "/A");
        test_serializer(TEnum::B(10, 20), "10 20 B");
    }
}

//! Deserialize JSON data to a Rust data structure

use core::str::FromStr;
use core::{fmt, str};

use serde::de::{self, Visitor};

use self::enum_::UnitVariantAccess;
use self::map::MapAccess;
use self::seq::SeqAccess;

mod enum_;
mod map;
mod seq;
mod string_unescaper;

/// Deserialization result
pub type Result<T> = core::result::Result<T, Error>;

/// This type represents all possible errors that can occur when deserializing JSON data
#[derive(Debug, PartialEq)]
pub enum Error {
    /// EOF while parsing a list.
    EofWhileParsingList,

    /// EOF while parsing an object.
    EofWhileParsingObject,

    /// EOF while parsing a string.
    EofWhileParsingString,

    /// EOF while parsing a JSON number.
    EofWhileParsingNumber,

    /// EOF while parsing a JSON value.
    EofWhileParsingValue,

    /// Expected this character to be a `':'`.
    ExpectedColon,

    /// Expected this character to be either a `','` or a `']'`.
    ExpectedListCommaOrEnd,

    /// Expected this character to be either a `','` or a `'}'`.
    ExpectedObjectCommaOrEnd,

    /// Expected to parse either a `true`, `false`, or a `null`.
    ExpectedSomeIdent,

    /// Expected this character to start a JSON value.
    ExpectedSomeValue,

    /// Invalid number.
    InvalidNumber,

    /// Invalid type
    InvalidType,

    /// Invalid unicode code point.
    InvalidUnicodeCodePoint,

    /// Object key is not a string.
    KeyMustBeAString,

    /// JSON has non-whitespace trailing characters after the value.
    TrailingCharacters,

    /// JSON has a comma after the last value in an array or map.
    TrailingComma,

    /// Attempted to decode an invalid escape within a string
    InvalidEscape,

    /// Error with a custom message that we had to discard.
    CustomError,

    /// Error with a custom message that was preserved.
    #[cfg(feature = "custom-error-messages")]
    CustomErrorWithMessage(heapless::String<heapless::consts::U64>),

    #[doc(hidden)]
    __Extensible,
}

#[cfg(feature = "std")]
impl ::std::error::Error for Error {
    fn description(&self) -> &str {
        ""
    }
}

pub(crate) struct Deserializer<'b> {
    slice: &'b mut [u8],
    index: usize,
}

impl<'a> Deserializer<'a> {
    fn new(slice: &'a mut [u8]) -> Deserializer<'_> {
        Deserializer { slice, index: 0 }
    }

    fn eat_char(&mut self) {
        self.index += 1;
    }

    fn end(&mut self) -> Result<()> {
        match self.parse_whitespace() {
            Some(_) => Err(Error::TrailingCharacters),
            None => Ok(()),
        }
    }

    fn end_seq(&mut self) -> Result<()> {
        match self.parse_whitespace().ok_or(Error::EofWhileParsingList)? {
            b']' => {
                self.eat_char();
                Ok(())
            }
            b',' => {
                self.eat_char();
                match self.parse_whitespace() {
                    Some(b']') => Err(Error::TrailingComma),
                    _ => Err(Error::TrailingCharacters),
                }
            }
            _ => Err(Error::TrailingCharacters),
        }
    }

    fn end_map(&mut self) -> Result<()> {
        match self
            .parse_whitespace()
            .ok_or(Error::EofWhileParsingObject)?
        {
            b'}' => {
                self.eat_char();
                Ok(())
            }
            b',' => Err(Error::TrailingComma),
            _ => Err(Error::TrailingCharacters),
        }
    }

    fn next_char(&mut self) -> Option<u8> {
        let ch = self.slice.get(self.index);

        if ch.is_some() {
            self.index += 1;
        }

        ch.cloned()
    }

    fn parse_ident(&mut self, ident: &[u8]) -> Result<()> {
        for c in ident {
            if Some(*c) != self.next_char() {
                return Err(Error::ExpectedSomeIdent);
            }
        }

        Ok(())
    }

    fn parse_object_colon(&mut self) -> Result<()> {
        match self
            .parse_whitespace()
            .ok_or(Error::EofWhileParsingObject)?
        {
            b':' => {
                self.eat_char();
                Ok(())
            }
            _ => Err(Error::ExpectedColon),
        }
    }

    fn parse_str(&mut self) -> Result<&'a str> {
        self.consume_passed();
        let (string_end, quote) = string_unescaper::unescape_string_to_termination(self.slice)?;
        self.index = string_end;
        let string_slice = self.consume_passed();
        self.index = quote - string_end;
        return str::from_utf8(string_slice)
            .map_err(|_| Error::InvalidUnicodeCodePoint);
    }

    fn consume_passed(&mut self) -> &'a mut [u8] {
        let len = self.slice.len();
        let ptr = self.slice.as_mut_ptr();

        unsafe {
            let passed = core::slice::from_raw_parts_mut(ptr, self.index);
            self.slice = core::slice::from_raw_parts_mut(ptr.add(self.index), len - self.index);
            self.index = 0;
            passed
        }
    }

    /// Consumes all the whitespace characters and returns a peek into the next character
    fn parse_whitespace(&mut self) -> Option<u8> {
        loop {
            match self.peek() {
                Some(b' ') | Some(b'\n') | Some(b'\t') | Some(b'\r') => {
                    self.eat_char();
                }
                other => {
                    return other;
                }
            }
        }
    }

    fn peek(&mut self) -> Option<u8> {
        self.slice.get(self.index).cloned()
    }
}

// NOTE(deserialize_*signed) we avoid parsing into u64 and then casting to a smaller integer, which
// is what upstream does, to avoid pulling in 64-bit compiler intrinsics, which waste a few KBs of
// Flash, when targeting non 64-bit architectures
macro_rules! deserialize_unsigned {
    ($self:ident, $visitor:ident, $uxx:ident, $visit_uxx:ident) => {{
        let peek = $self
            .parse_whitespace()
            .ok_or(Error::EofWhileParsingValue)?;

        match peek {
            b'-' => Err(Error::InvalidNumber),
            b'0' => {
                $self.eat_char();
                $visitor.$visit_uxx(0)
            }
            b'1'..=b'9' => {
                $self.eat_char();

                let mut number = (peek - b'0') as $uxx;
                loop {
                    match $self.peek() {
                        Some(c @ b'0'..=b'9') => {
                            $self.eat_char();
                            number = number
                                .checked_mul(10)
                                .ok_or(Error::InvalidNumber)?
                                .checked_add((c - b'0') as $uxx)
                                .ok_or(Error::InvalidNumber)?;
                        }
                        _ => return $visitor.$visit_uxx(number),
                    }
                }
            }
            _ => Err(Error::InvalidType),
        }
    }};
}

macro_rules! deserialize_signed {
    ($self:ident, $visitor:ident, $ixx:ident, $visit_ixx:ident) => {{
        let signed = match $self
            .parse_whitespace()
            .ok_or(Error::EofWhileParsingValue)?
        {
            b'-' => {
                $self.eat_char();
                true
            }
            _ => false,
        };

        match $self.peek().ok_or(Error::EofWhileParsingValue)? {
            b'0' => {
                $self.eat_char();
                $visitor.$visit_ixx(0)
            }
            c @ b'1'..=b'9' => {
                $self.eat_char();

                let mut number = (c - b'0') as $ixx * if signed { -1 } else { 1 };
                loop {
                    match $self.peek() {
                        Some(c @ b'0'..=b'9') => {
                            $self.eat_char();
                            number = number
                                .checked_mul(10)
                                .ok_or(Error::InvalidNumber)?
                                .checked_add((c - b'0') as $ixx * if signed { -1 } else { 1 })
                                .ok_or(Error::InvalidNumber)?;
                        }
                        _ => return $visitor.$visit_ixx(number),
                    }
                }
            }
            _ => return Err(Error::InvalidType),
        }
    }};
}

macro_rules! deserialize_fromstr {
    ($self:ident, $visitor:ident, $typ:ident, $visit_fn:ident, $pattern:expr) => {{
        let start = $self.index;
        loop {
            match $self.peek() {
                Some(c) => {
                    if $pattern.iter().find(|&&d| d == c).is_some() {
                        $self.eat_char();
                    } else {
                        let s = unsafe {
                            // already checked that it contains only ascii
                            str::from_utf8_unchecked(&$self.slice[start..$self.index])
                        };
                        let v = $typ::from_str(s).or(Err(Error::InvalidNumber))?;
                        return $visitor.$visit_fn(v);
                    }
                }
                None => return Err(Error::EofWhileParsingNumber),
            }
        }
    }};
}

impl<'a, 'de> de::Deserializer<'de> for &'a mut Deserializer<'de> {
    type Error = Error;

    /// Unsupported. Can’t parse a value without knowing its expected type.
    fn deserialize_any<V>(self, _visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        unreachable!()
    }

    fn deserialize_bool<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        let peek = self.parse_whitespace().ok_or(Error::EofWhileParsingValue)?;

        match peek {
            b't' => {
                self.eat_char();
                self.parse_ident(b"rue")?;
                visitor.visit_bool(true)
            }
            b'f' => {
                self.eat_char();
                self.parse_ident(b"alse")?;
                visitor.visit_bool(false)
            }
            _ => Err(Error::InvalidType),
        }
    }

    fn deserialize_i8<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        deserialize_signed!(self, visitor, i8, visit_i8)
    }

    fn deserialize_i16<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        deserialize_signed!(self, visitor, i16, visit_i16)
    }

    fn deserialize_i32<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        deserialize_signed!(self, visitor, i32, visit_i32)
    }

    fn deserialize_i64<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        deserialize_signed!(self, visitor, i64, visit_i64)
    }

    fn deserialize_u8<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        deserialize_unsigned!(self, visitor, u8, visit_u8)
    }

    fn deserialize_u16<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        deserialize_unsigned!(self, visitor, u16, visit_u16)
    }

    fn deserialize_u32<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        deserialize_unsigned!(self, visitor, u32, visit_u32)
    }

    fn deserialize_u64<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        deserialize_unsigned!(self, visitor, u64, visit_u64)
    }

    fn deserialize_f32<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        self.parse_whitespace().ok_or(Error::EofWhileParsingValue)?;
        deserialize_fromstr!(self, visitor, f32, visit_f32, b"0123456789+-.eE")
    }

    fn deserialize_f64<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        self.parse_whitespace().ok_or(Error::EofWhileParsingValue)?;
        deserialize_fromstr!(self, visitor, f64, visit_f64, b"0123456789+-.eE")
    }

    fn deserialize_char<V>(self, _visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        unreachable!()
    }

    fn deserialize_str<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        let peek = self.parse_whitespace().ok_or(Error::EofWhileParsingValue)?;

        match peek {
            b'"' => {
                self.eat_char();
                visitor.visit_borrowed_str(self.parse_str()?)
            }
            _ => Err(Error::InvalidType),
        }
    }

    /// Unsupported. String is not available in no-std.
    fn deserialize_string<V>(self, _visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        unreachable!()
    }

    /// Unsupported
    fn deserialize_bytes<V>(self, _visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        unreachable!()
    }

    /// Unsupported
    fn deserialize_byte_buf<V>(self, _visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        unreachable!()
    }

    fn deserialize_option<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        match self.parse_whitespace().ok_or(Error::EofWhileParsingValue)? {
            b'n' => {
                self.eat_char();
                self.parse_ident(b"ull")?;
                visitor.visit_none()
            }
            _ => visitor.visit_some(self),
        }
    }

    /// Unsupported. Use a more specific deserialize_* method
    fn deserialize_unit<V>(self, _visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        unreachable!()
    }

    /// Unsupported. Use a more specific deserialize_* method
    fn deserialize_unit_struct<V>(self, _name: &'static str, _visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        unreachable!()
    }

    /// Unsupported. We can’t parse newtypes because we don’t know the underlying type.
    fn deserialize_newtype_struct<V>(self, _name: &'static str, _visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        unreachable!()
    }

    fn deserialize_seq<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        match self.peek().ok_or(Error::EofWhileParsingValue)? {
            b'[' => {
                self.eat_char();
                let ret = visitor.visit_seq(SeqAccess::new(self))?;

                self.end_seq()?;

                Ok(ret)
            }
            _ => Err(Error::InvalidType),
        }
    }

    fn deserialize_tuple<V>(self, _len: usize, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        self.deserialize_seq(visitor)
    }

    fn deserialize_tuple_struct<V>(
        self,
        _name: &'static str,
        _len: usize,
        visitor: V,
    ) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        self.deserialize_seq(visitor)
    }

    fn deserialize_map<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        let peek = self.parse_whitespace().ok_or(Error::EofWhileParsingValue)?;

        if peek == b'{' {
            self.eat_char();

            let ret = visitor.visit_map(MapAccess::new(self))?;

            self.end_map()?;

            Ok(ret)
        } else {
            Err(Error::InvalidType)
        }
    }

    fn deserialize_struct<V>(
        self,
        _name: &'static str,
        _fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        self.deserialize_map(visitor)
    }

    fn deserialize_enum<V>(
        self,
        _name: &'static str,
        _variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        match self.parse_whitespace().ok_or(Error::EofWhileParsingValue)? {
            b'"' => visitor.visit_enum(UnitVariantAccess::new(self)),
            _ => Err(Error::ExpectedSomeValue),
        }
    }

    fn deserialize_identifier<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        self.deserialize_str(visitor)
    }

    /// Used to throw out fields from JSON objects that we don’t want to
    /// keep in our structs.
    fn deserialize_ignored_any<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        match self.parse_whitespace().ok_or(Error::EofWhileParsingValue)? {
            b'"' => self.deserialize_str(visitor),
            b'[' => self.deserialize_seq(visitor),
            b'{' => self.deserialize_struct("ignored", &[], visitor),
            b',' | b'}' | b']' => Err(Error::ExpectedSomeValue),
            // If it’s something else then we chomp until we get to an end delimiter.
            // This does technically allow for illegal JSON since we’re just ignoring
            // characters rather than parsing them.
            _ => loop {
                match self.peek() {
                    // The visitor is expected to be UnknownAny’s visitor, which
                    // implements visit_unit to return its unit Ok result.
                    Some(b',') | Some(b'}') | Some(b']') => break visitor.visit_unit(),
                    Some(_) => self.eat_char(),
                    None => break Err(Error::EofWhileParsingString),
                }
            },
        }
    }
}

impl de::Error for Error {
    #[cfg_attr(not(feature = "custom-error-messages"), allow(unused_variables))]
    fn custom<T>(msg: T) -> Self
    where
        T: fmt::Display,
    {
        #[cfg(not(feature = "custom-error-messages"))]
        {
            Error::CustomError
        }
        #[cfg(feature = "custom-error-messages")]
        {
            use core::fmt::Write;

            let mut string = heapless::String::new();
            write!(string, "{:.64}", msg).unwrap();
            Error::CustomErrorWithMessage(string)
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}",
            match self {
                Error::EofWhileParsingList => "EOF while parsing a list.",
                Error::EofWhileParsingObject => "EOF while parsing an object.",
                Error::EofWhileParsingString => "EOF while parsing a string.",
                Error::EofWhileParsingValue => "EOF while parsing a JSON value.",
                Error::ExpectedColon => "Expected this character to be a `':'`.",
                Error::ExpectedListCommaOrEnd => {
                    "Expected this character to be either a `','` or\
                     a \
                     `']'`."
                }
                Error::ExpectedObjectCommaOrEnd => {
                    "Expected this character to be either a `','` \
                     or a \
                     `'}'`."
                }
                Error::ExpectedSomeIdent => {
                    "Expected to parse either a `true`, `false`, or a \
                     `null`."
                }
                Error::ExpectedSomeValue => "Expected this character to start a JSON value.",
                Error::InvalidNumber => "Invalid number.",
                Error::InvalidType => "Invalid type",
                Error::InvalidUnicodeCodePoint => "Invalid unicode code point.",
                Error::KeyMustBeAString => "Object key is not a string.",
                Error::TrailingCharacters => {
                    "JSON has non-whitespace trailing characters after \
                     the \
                     value."
                }
                Error::TrailingComma => "JSON has a comma after the last value in an array or map.",
                Error::CustomError => "JSON does not match deserializer’s expected format.",
                #[cfg(feature = "custom-error-messages")]
                Error::CustomErrorWithMessage(msg) => msg.as_str(),
                _ => "Invalid JSON",
            }
        )
    }
}

/// Deserializes an instance of type `T` from bytes of JSON text
pub fn from_slice<'a, T>(v: &'a mut [u8]) -> Result<T>
where
    T: de::Deserialize<'a>,
{
    let mut de = Deserializer::new(v);
    let value = de::Deserialize::deserialize(&mut de)?;
    de.end()?;

    Ok(value)
}

/// Deserializes an instance of type T from a string of JSON text
pub fn from_str<'a, T>(s: &'a mut str) -> Result<T>
where
    T: de::Deserialize<'a>,
{
    unsafe {
        from_slice(s.as_bytes_mut())
    }
}

#[cfg(test)]
mod tests {
    use serde_derive::Deserialize;
    use heapless::consts::*;
    use heapless::String;
    use core::str::FromStr;

    #[derive(Debug, Deserialize, PartialEq)]
    enum Type {
        #[serde(rename = "boolean")]
        Boolean,
        #[serde(rename = "number")]
        Number,
        #[serde(rename = "thing")]
        Thing,
    }

    #[test]
    fn array() {
        assert_eq!(crate::from_str::<[i32; 0]>(&mut String::<U64>::from_str("[]").unwrap()), Ok([]));
        assert_eq!(crate::from_str(&mut String::<U64>::from_str("[0, 1, 2]").unwrap()), Ok([0, 1, 2]));

        // errors
        assert!(crate::from_str::<[i32; 2]>(&mut String::<U64>::from_str("[0, 1,]").unwrap()).is_err());
    }

    #[test]
    fn bool() {
        assert_eq!(crate::from_str(&mut String::<U64>::from_str("true").unwrap()), Ok(true));
        assert_eq!(crate::from_str(&mut String::<U64>::from_str(" true").unwrap()), Ok(true));
        assert_eq!(crate::from_str(&mut String::<U64>::from_str("true ").unwrap()), Ok(true));

        assert_eq!(crate::from_str(&mut String::<U64>::from_str("false").unwrap()), Ok(false));
        assert_eq!(crate::from_str(&mut String::<U64>::from_str(" false").unwrap()), Ok(false));
        assert_eq!(crate::from_str(&mut String::<U64>::from_str("false ").unwrap()), Ok(false));

        // errors
        assert!(crate::from_str::<bool>(&mut String::<U64>::from_str("true false").unwrap()).is_err());
        assert!(crate::from_str::<bool>(&mut String::<U64>::from_str("tru").unwrap()).is_err());
    }

    #[test]
    fn enum_clike() {
        assert_eq!(crate::from_str(&mut String::<U64>::from_str(r#" "boolean" "#).unwrap()), Ok(Type::Boolean));
        assert_eq!(crate::from_str(&mut String::<U64>::from_str(r#" "number" "#).unwrap()), Ok(Type::Number));
        assert_eq!(crate::from_str(&mut String::<U64>::from_str(r#" "thing" "#).unwrap()), Ok(Type::Thing));
    }

    #[test]
    fn str() {
        assert_eq!(crate::from_str(&mut String::<U64>::from_str(r#" "hello" "#).unwrap()), Ok("hello"));
    }

    #[test]
    fn escaped_str() {
        assert_eq!(crate::from_str(&mut String::<U64>::from_str(r#" "\\test\/\"  \u263A" "#).unwrap()), Ok(r#"\test/"  ☺"#));
    }

    #[test]
    fn struct_bool() {
        #[derive(Debug, Deserialize, PartialEq)]
        struct Led {
            led: bool,
        }

        assert_eq!(crate::from_str(&mut String::<U64>::from_str(r#"{ "led": true }"#).unwrap()), Ok(Led { led: true }));
        assert_eq!(
            crate::from_str(&mut String::<U64>::from_str(r#"{ "led": false }"#).unwrap()),
            Ok(Led { led: false })
        );
    }

    #[test]
    fn struct_i8() {
        #[derive(Debug, Deserialize, PartialEq)]
        struct Temperature {
            temperature: i8,
        }

        assert_eq!(
            crate::from_str(&mut String::<U64>::from_str(r#"{ "temperature": -17 }"#).unwrap()),
            Ok(Temperature { temperature: -17 })
        );

        assert_eq!(
            crate::from_str(&mut String::<U64>::from_str(r#"{ "temperature": -0 }"#).unwrap()),
            Ok(Temperature { temperature: -0 })
        );

        assert_eq!(
            crate::from_str(&mut String::<U64>::from_str(r#"{ "temperature": 0 }"#).unwrap()),
            Ok(Temperature { temperature: 0 })
        );

        // out of range
        assert!(crate::from_str::<Temperature>(&mut String::<U64>::from_str(r#"{ "temperature": 128 }"#).unwrap()).is_err());
        assert!(crate::from_str::<Temperature>(&mut String::<U64>::from_str(r#"{ "temperature": -129 }"#).unwrap()).is_err());
    }

    #[test]
    fn struct_f32() {
        #[derive(Debug, Deserialize, PartialEq)]
        struct Temperature {
            temperature: f32,
        }

        assert_eq!(
            crate::from_str(&mut String::<U64>::from_str(r#"{ "temperature": -17.2 }"#).unwrap()),
            Ok(Temperature { temperature: -17.2 })
        );

        assert_eq!(
            crate::from_str(&mut String::<U64>::from_str(r#"{ "temperature": -0.0 }"#).unwrap()),
            Ok(Temperature { temperature: -0. })
        );

        assert_eq!(
            crate::from_str(&mut String::<U64>::from_str(r#"{ "temperature": -2.1e-3 }"#).unwrap()),
            Ok(Temperature {
                temperature: -2.1e-3
            })
        );

        assert_eq!(
            crate::from_str(&mut String::<U64>::from_str(r#"{ "temperature": -3 }"#).unwrap()),
            Ok(Temperature { temperature: -3. })
        );

        use core::f32;

        assert_eq!(
            crate::from_str(&mut String::<U64>::from_str(r#"{ "temperature": -1e500 }"#).unwrap()),
            Ok(Temperature {
                temperature: f32::NEG_INFINITY
            })
        );

        assert!(crate::from_str::<Temperature>(&mut String::<U64>::from_str(r#"{ "temperature": 1e1e1 }"#).unwrap()).is_err());
        assert!(crate::from_str::<Temperature>(&mut String::<U64>::from_str(r#"{ "temperature": -2-2 }"#).unwrap()).is_err());
        assert!(crate::from_str::<Temperature>(&mut String::<U64>::from_str(r#"{ "temperature": 1 1 }"#).unwrap()).is_err());
        assert!(crate::from_str::<Temperature>(&mut String::<U64>::from_str(r#"{ "temperature": 0.0. }"#).unwrap()).is_err());
        assert!(crate::from_str::<Temperature>(&mut String::<U64>::from_str(r#"{ "temperature": ä }"#).unwrap()).is_err());
    }

    #[test]
    fn struct_option() {
        #[derive(Debug, Deserialize, PartialEq)]
        struct Property<'a> {
            #[serde(borrow)]
            description: Option<&'a str>,
        }

        assert_eq!(
            crate::from_str(&mut String::<U64>::from_str(r#"{ "description": "An ambient temperature sensor" }"#).unwrap()),
            Ok(Property {
                description: Some("An ambient temperature sensor"),
            })
        );

        assert_eq!(
            crate::from_str(&mut String::<U64>::from_str(r#"{ "description": null }"#).unwrap()),
            Ok(Property { description: None })
        );

        assert_eq!(crate::from_str(&mut String::<U64>::from_str(r#"{}"#).unwrap()), Ok(Property { description: None }));
    }

    #[test]
    fn struct_u8() {
        #[derive(Debug, Deserialize, PartialEq)]
        struct Temperature {
            temperature: u8,
        }

        assert_eq!(
            crate::from_str(&mut String::<U64>::from_str(r#"{ "temperature": 20 }"#).unwrap()),
            Ok(Temperature { temperature: 20 })
        );

        assert_eq!(
            crate::from_str(&mut String::<U64>::from_str(r#"{ "temperature": 0 }"#).unwrap()),
            Ok(Temperature { temperature: 0 })
        );

        // out of range
        assert!(crate::from_str::<Temperature>(&mut String::<U64>::from_str(r#"{ "temperature": 256 }"#).unwrap()).is_err());
        assert!(crate::from_str::<Temperature>(&mut String::<U64>::from_str(r#"{ "temperature": -1 }"#).unwrap()).is_err());
    }

    #[test]
    #[cfg(not(feature = "custom-error-messages"))]
    fn struct_tuple() {
        #[derive(Debug, Deserialize, PartialEq)]
        struct Xy(i8, i8);

        assert_eq!(crate::from_str(&mut String::<U64>::from_str(r#"[10, 20]"#).unwrap()), Ok(Xy(10, 20)));
        assert_eq!(crate::from_str(&mut String::<U64>::from_str(r#"[10, -20]"#).unwrap()), Ok(Xy(10, -20)));

        // wrong number of args
        assert_eq!(
            crate::from_str::<Xy>(&mut String::<U64>::from_str(r#"[10]"#).unwrap()),
            Err(crate::de::Error::CustomError)
        );
        assert_eq!(
            crate::from_str::<Xy>(&mut String::<U64>::from_str(r#"[10, 20, 30]"#).unwrap()),
            Err(crate::de::Error::TrailingCharacters)
        );
    }

    #[test]
    #[cfg(feature = "custom-error-messages")]
    fn struct_tuple() {
        #[derive(Debug, Deserialize, PartialEq)]
        struct Xy(i8, i8);

        assert_eq!(crate::from_str(&mut String::<U64>::from_str(r#"[10, 20]"#).unwrap()), Ok(Xy(10, 20)));
        assert_eq!(crate::from_str(&mut String::<U64>::from_str(r#"[10, -20]"#).unwrap()), Ok(Xy(10, -20)));

        // wrong number of args
        assert_eq!(
            crate::from_str::<Xy>(&mut String::<U64>::from_str(r#"[10]"#).unwrap()),
            Err(crate::de::Error::CustomErrorWithMessage(
                "invalid length 1, expected tuple struct Xy with 2 elements".into()
            ))
        );
        assert_eq!(
            crate::from_str::<Xy>(&mut String::<U64>::from_str(r#"[10, 20, 30]"#).unwrap()),
            Err(crate::de::Error::TrailingCharacters)
        );
    }

    #[test]
    fn ignoring_extra_fields() {
        #[derive(Debug, Deserialize, PartialEq)]
        struct Temperature {
            temperature: u8,
        }

        assert_eq!(
            crate::from_str(&mut String::<U64>::from_str(r#"{ "temperature": 20, "high": 80, "low": -10, "updated": true }"#).unwrap()),
            Ok(Temperature { temperature: 20 })
        );

        assert_eq!(
            crate::from_str(&mut String::<U128>::from_str(
                r#"{ "temperature": 20, "conditions": "windy", "forecast": "cloudy" }"#
            ).unwrap()),
            Ok(Temperature { temperature: 20 })
        );

        assert_eq!(
            crate::from_str(&mut String::<U128>::from_str(r#"{ "temperature": 20, "hourly_conditions": ["windy", "rainy"] }"#).unwrap()),
            Ok(Temperature { temperature: 20 })
        );

        assert_eq!(
            crate::from_str(&mut String::<U128>::from_str(
                r#"{ "temperature": 20, "source": { "station": "dock", "sensors": ["front", "back"] } }"#
            ).unwrap()),
            Ok(Temperature { temperature: 20 })
        );

        assert_eq!(
            crate::from_str(&mut String::<U128>::from_str(r#"{ "temperature": 20, "invalid": this-is-ignored }"#).unwrap()),
            Ok(Temperature { temperature: 20 })
        );

        assert_eq!(
            crate::from_str::<Temperature>(&mut String::<U64>::from_str(r#"{ "temperature": 20, "broken": }"#).unwrap()),
            Err(crate::de::Error::ExpectedSomeValue)
        );

        assert_eq!(
            crate::from_str::<Temperature>(&mut String::<U64>::from_str(r#"{ "temperature": 20, "broken": [ }"#).unwrap()),
            Err(crate::de::Error::ExpectedSomeValue)
        );

        assert_eq!(
            crate::from_str::<Temperature>(&mut String::<U64>::from_str(r#"{ "temperature": 20, "broken": ] }"#).unwrap()),
            Err(crate::de::Error::ExpectedSomeValue)
        );
    }

    #[test]
    #[cfg(feature = "custom-error-messages")]
    fn preserve_short_error_message() {
        use serde::de::Error;
        assert_eq!(
            crate::de::Error::custom("something bad happened"),
            crate::de::Error::CustomErrorWithMessage("something bad happened".into())
        );
    }

    #[test]
    #[cfg(feature = "custom-error-messages")]
    fn truncate_error_message() {
        use serde::de::Error;
        assert_eq!(
            crate::de::Error::custom("0123456789012345678901234567890123456789012345678901234567890123 <- after here the message should be truncated"),
            crate::de::Error::CustomErrorWithMessage(
                "0123456789012345678901234567890123456789012345678901234567890123".into()
            )
        );
    }

    // See https://iot.mozilla.org/wot/#thing-resource
    #[test]
    #[ignore]
    fn wot() {
        #[derive(Debug, Deserialize, PartialEq)]
        struct Thing<'a> {
            #[serde(borrow)]
            properties: Properties<'a>,
            #[serde(rename = "type")]
            ty: Type,
        }

        #[derive(Debug, Deserialize, PartialEq)]
        struct Properties<'a> {
            #[serde(borrow)]
            temperature: Property<'a>,
            #[serde(borrow)]
            humidity: Property<'a>,
            #[serde(borrow)]
            led: Property<'a>,
        }

        #[derive(Debug, Deserialize, PartialEq)]
        struct Property<'a> {
            #[serde(rename = "type")]
            ty: Type,
            unit: Option<&'a str>,
            #[serde(borrow)]
            description: Option<&'a str>,
            href: &'a str,
        }

        let mut string = String::<U128>::from_str(r#"
{
  "type": "thing",
  "properties": {
    "temperature": {
      "type": "number",
      "unit": "celsius",
      "description": "An ambient temperature sensor",
      "href": "/properties/temperature"
    },
    "humidity": {
      "type": "number",
      "unit": "percent",
      "href": "/properties/humidity"
    },
    "led": {
      "type": "boolean",
      "description": "A red LED",
      "href": "/properties/led"
    }
  }
}
"#).unwrap();

    assert_eq!(crate::from_str::<Thing<'_>>(&mut string),
            Ok(Thing {
                properties: Properties {
                    temperature: Property {
                        ty: Type::Number,
                        unit: Some("celcius"),
                        description: Some("An ambient temperature sensor"),
                        href: "/properties/temperature",
                    },
                    humidity: Property {
                        ty: Type::Number,
                        unit: Some("percent"),
                        description: None,
                        href: "/properties/humidity",
                    },
                    led: Property {
                        ty: Type::Boolean,
                        unit: None,
                        description: Some("A red LED"),
                        href: "/properties/led",
                    },
                },
                ty: Type::Thing,
            })
        )
    }
}

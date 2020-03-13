use core::str;
use super::Result;
use super::Error;

/// parses a string
pub fn parse_string<'a>(buffer: &'a mut [u8], start: usize, end: usize) -> Result<&'a mut str> {
    let string = str::from_utf8_mut(&mut buffer[start..end]).map_err(|_| Error::InvalidUnicodeCodePoint)?;
    mutate(string)
}

/// Unescapes a JSON string in place, returning a new slice which may be shorter than the input
/// # Errors
/// `InvalidEscape` when either:
///     * an escape character '\' is not followed by one of the accepted JSON escape codes '\', '/', '"', 'u'
///     * a unicode escape '\u' is not followed by exactly 4 hex digits
/// 
/// # Panics
/// * If an unescaped double quote is encountered, this suggests the caller missed the end of the string
/// * If an escape character is the last character of the string, this suggestes the caller has missed the escape
/// 
/// # Unsafe
/// This function uses unsafe code, 
fn mutate<'a>(string: &'a mut str) -> Result<&'a mut str> {
    let mut w = 0;
    unsafe {
        let mut r = 0;
        while r < string.len() {
            let read_byte = string.as_bytes()[r];
            r += 1;
            if read_byte == b'\\' {
                let escaped_byte = string.as_bytes()[r];
                r += 1;
                match escaped_byte {
                    b'\\' | b'/' | b'"' => {
                        let byte = &mut string.as_bytes_mut()[w];
                        w += 1;
                        *byte = escaped_byte;
                    },
                    b'u' => {
                        let codepoint = u32::from_str_radix(&string[r..r+4], 16).map_err(|_| Error::InvalidEscape)?;
                        let codepoint = core::char::from_u32(codepoint).unwrap(); // Should never get an invalid char from 4 hex digits
                        let encoded_string = codepoint.encode_utf8(&mut string.as_bytes_mut()[w..]);
                        r += 4;
                        w += encoded_string.len();
                    },
                    _bad_escape => Err(Error::InvalidEscape)?,
                }
            } else if read_byte == b'"' {
                panic!("Unescaped quote in string"); // the caller should have treated this as the end of the string
            } else {
                let byte = &mut string.as_bytes_mut()[w];
                w += 1;
                *byte = read_byte;
            }
        }
    }
    return Ok(&mut string[0..w])
}

#[cfg(test)]
mod tests {
    use super::parse_string;
    use super::super::Error;
    use core::convert::TryFrom;

    #[test]
    fn unicode() {
            let mut b: [u8; 4] = <[u8; 4]>::try_from(" ☺".as_bytes()).unwrap();
            let l = b.len();
            assert_eq!(parse_string(b.as_mut(), 1, l).unwrap(), "☺");
    }
    
    #[test]
    fn escape_literals() {
        let mut slice = b"  \\\\test\\/\\\" ".clone();
        let l = slice.len();
        assert_eq!(parse_string(slice.as_mut(), 1, l).unwrap(), " \\test/\" ");
    }

    #[test]
    fn escape_unicode() {
        let mut slice = b" \\u263A".clone();
        let l = slice.len();
        assert_eq!(parse_string(slice.as_mut(), 1, l).unwrap(), "☺");
    }

    #[test]
    fn escape_unicode_invalid_hex() {
        let mut slice = b" \\uASDF".clone();
        let l = slice.len();
        assert_eq!(parse_string(slice.as_mut(), 1, l), Err(Error::InvalidEscape));
    }

    #[test]
    fn escape_unicode_noncharacter() {
        let mut slice = b" \\uFFFF".clone();
        let l = slice.len();
        assert_eq!(parse_string(slice.as_mut(), 1, l).unwrap(), "\u{FFFF}");
    }

    #[test]
    fn escape_invalid() {
        let mut slice = b" \\E".clone();
        let l = slice.len();
        assert_eq!(parse_string(slice.as_mut(), 1, l), Err(Error::InvalidEscape));
    }

    #[test]
    #[should_panic]
    fn escape_at_end() {
        let mut slice = b" \\".clone();
        let l = slice.len();
        let _result = parse_string(slice.as_mut(), 1, l);
    }

    #[test]
    #[should_panic]
    fn unescaped_double_quote() {
        let mut slice = b" \"".clone();
        let l = slice.len();
        let _result = parse_string(slice.as_mut(), 1, l);
    }
}

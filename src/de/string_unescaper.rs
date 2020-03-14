use core::str;
use super::Result;
use super::Error;

/// Unescapes a JSON string in place up until the terminating quote, returning the length of the unescaped string from the start of the buffer and the position where parsing should continue
/// # Errors
/// `InvalidEscape` when either:
///     * an escape character '\' is not followed by one of the accepted JSON escape codes '\', '/', '"', 'u'
///     * a unicode escape '\u' is not followed by exactly 4 hex digits
/// `EofWhileParsingString` when the buffer end is reached without finding a terminating quote
pub fn unescape_string_to_termination(buffer: &mut [u8]) -> Result<(usize, usize)> {
    let mut w = 0;
    let mut r = 0;
    loop {
        if buffer.len() < r + 1 {
            return Err(Error::EofWhileParsingString)
        }
        let read_byte = buffer[r];
        r += 1;
        if read_byte == b'\\' { // I believe this (checking for the escape character in u8s and not worrying about longer characters) is safe to do as the UTF-8 character code for this (5C) is never used as part of another UTF-8 character
            if buffer.len() < r + 1 {
                return Err(Error::EofWhileParsingString)
            }
            let escaped_byte = buffer[r];
            r += 1;
            match escaped_byte {
                b'\\' | b'/' | b'"' => {
                    buffer[w] = escaped_byte;
                    w += 1;
                },
                b'u' => {
                    if buffer.len() < r + 4 {
                        return Err(Error::EofWhileParsingString)
                    }
                    let codepoint_string = str::from_utf8(&buffer[r..r+4]).map_err(|_| Error::InvalidEscape)?;
                    let codepoint = u32::from_str_radix(codepoint_string, 16).map_err(|_| Error::InvalidEscape)?;
                    let codepoint = core::char::from_u32(codepoint).unwrap(); // Should never get an invalid char from 4 hex digits
                    let encoded_string = codepoint.encode_utf8(&mut buffer[w..]);
                    r += 4;
                    w += encoded_string.len();
                },
                _bad_escape => Err(Error::InvalidEscape)?,
            }
        } else if read_byte == b'"' {
            break;
        } else {
            buffer[w] = read_byte;
            w += 1;
        }
    }
    return Ok((w, r))
}

#[cfg(test)]
mod tests {
    use super::unescape_string_to_termination;
    use super::super::Error;
    use core::convert::TryFrom;
    use core::str;
    use super::super::Result;

    /// parses a string
    fn parse_string<'a>(buffer: &'a mut [u8]) -> Result<(&'a str, usize)> {
        let (string_end, next) = unescape_string_to_termination(buffer)?;
        let unescaped = str::from_utf8_mut(&mut buffer[..string_end]).unwrap();
        Ok((unescaped, next))
    }

    #[test]
    fn unicode() {
            let mut b = <[u8; 5]>::try_from(r#"☺" "#.as_bytes()).unwrap();
            assert_eq!(parse_string(b.as_mut()).unwrap(), ("☺", 4));
    }
    
    #[test]
    fn escape_literals() {
        let mut slice = b" \\\\test\\/\\\" \" ".clone();
        let l = slice.len();
        assert_eq!(parse_string(slice.as_mut()).unwrap(), (r#" \test/" "#, l-1));
    }

    #[test]
    fn escape_unicode() {
        let mut slice = b"\\u263A\" ".clone();
        assert_eq!(parse_string(slice.as_mut()).unwrap(), ("☺", 7));
    }

    #[test]
    fn escape_unicode_invalid_hex() {
        let mut slice = b"\\uASDF".clone();
        assert_eq!(parse_string(slice.as_mut()), Err(Error::InvalidEscape));
    }

    #[test]
    fn escape_unicode_noncharacter() {
        let mut slice = b"\\uFFFF\" ".clone();
        let l = slice.len();
        assert_eq!(parse_string(slice.as_mut()).unwrap(), ("\u{FFFF}", l - 1));
    }

    #[test]
    fn escape_invalid() {
        let mut slice = b"\\E".clone();
        assert_eq!(parse_string(slice.as_mut()), Err(Error::InvalidEscape));
    }

    #[test]
    fn escape_at_end() {
        let mut slice = b"\\".clone();
        assert_eq!(parse_string(slice.as_mut()), Err(Error::EofWhileParsingString));
    }

    #[test]
    fn incomplete_unicode_at_end() {
        let mut slice = b"\\uFFF".clone();
        assert_eq!(parse_string(slice.as_mut()), Err(Error::EofWhileParsingString));
    }

    #[test]
    fn terminates_on_quote() {
        let mut slice = b"\" ".clone();
        assert_eq!(parse_string(slice.as_mut()).unwrap(), ("", 1));
    }
}

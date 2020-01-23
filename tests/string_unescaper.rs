use core::str;
use core::result::Result;

struct StringParser<S> (S);

impl <'a> StringParser<&'a [u8]> {
    fn parse_string(&'a self, start: usize, end: usize) -> Result<&'a str, str::Utf8Error> {
        return str::from_utf8(&self.0[start..end])
    }
}

impl <'a> StringParser<&'a mut [u8]> {
    fn parse_string(&'a mut self, start: usize, end: usize) -> Result<&'a str, str::Utf8Error> {
        let string = str::from_utf8_mut(&mut self.0[start..end]);
        return string.map(mutate)
    }
}

fn mutate(string: &mut str) -> &str {
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
                        println!("unicode U+{}", &string[r..r+4]);
                        let c = core::char::from_u32(u32::from_str_radix(&string[r..r+4], 16).unwrap()).unwrap();
                        let codepoint = c.encode_utf8(&mut string.as_bytes_mut()[w..]);
                        r += 4;
                        w += codepoint.len();
                    }
                    c => panic!("bad escape : {}", char::from(c)),
                }
            } else {
                let byte = &mut string.as_bytes_mut()[w];
                w += 1;
                *byte = read_byte;
            }
        }
    }
    return &string[..w]
}

#[test]
fn string() {
    let slice = b" \\\\test\\\\".clone();
    let l = slice.len();
    assert_eq!(StringParser(slice.as_ref()).parse_string(1, l).unwrap(), "\\\\test\\\\");
}

#[test]
fn string_escape_literals() {
    let mut slice = b"  \\\\test\\/\\\" ".clone();
    let l = slice.len();
    assert_eq!(StringParser(slice.as_mut()).parse_string(1, l).unwrap(), " \\test/\" ");
}

#[test]
fn string_escape_unicode() {
    let mut slice = b" \\u263A".clone();
    let l = slice.len();
    assert_eq!(StringParser(slice.as_mut()).parse_string(1, l).unwrap(), "☺");
}

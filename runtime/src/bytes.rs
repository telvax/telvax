//! The functions behind `std.bytes`, `std.encoding` and `std.crypto`, on plain byte slices.

use sha2::{Digest, Sha256};

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Lower case hexadecimal, two digits for each byte.
pub fn hex_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(char::from(b"0123456789abcdef"[usize::from(b >> 4)]));
        out.push(char::from(b"0123456789abcdef"[usize::from(b & 15)]));
    }
    out
}

/// Hexadecimal in either case, an even number of digits and nothing else.
pub fn hex_decode(text: &str) -> Result<Vec<u8>, String> {
    let digits = text.as_bytes();
    if digits.len() % 2 != 0 {
        return Err(format!(
            "`{}` has {} digit(s); hexadecimal needs an even number",
            shorten(text),
            digits.len()
        ));
    }
    let value = |c: u8| match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    };
    digits
        .chunks(2)
        .enumerate()
        .map(|(i, pair)| match (value(pair[0]), value(pair[1])) {
            (Some(h), Some(l)) => Ok(h << 4 | l),
            _ => Err(format!(
                "`{}` is not hexadecimal: the digits at position {} are `{}`",
                shorten(text),
                i * 2,
                String::from_utf8_lossy(pair)
            )),
        })
        .collect()
}

/// Base64 with the standard alphabet and `=` padding (RFC 4648).
pub fn base64_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = u32::from(chunk[0]) << 16
            | u32::from(*chunk.get(1).unwrap_or(&0)) << 8
            | u32::from(*chunk.get(2).unwrap_or(&0));
        for k in 0..4 {
            if k <= chunk.len() {
                out.push(char::from(B64[((n >> (18 - 6 * k)) & 63) as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// Base64 with the standard alphabet and the padding (RFC 4648); anything else, line breaks included, is an error.
pub fn base64_decode(text: &str) -> Result<Vec<u8>, String> {
    let data = text.as_bytes();
    if data.len() % 4 != 0 {
        return Err(format!(
            "`{}` is not base64: its length is {}, and it has to be a multiple of 4 (with `=` at the end)",
            shorten(text),
            data.len()
        ));
    }
    let mut out = Vec::with_capacity(data.len() / 4 * 3);
    for (index, quad) in data.chunks(4).enumerate() {
        let last = (index + 1) * 4 == data.len();
        let pad = quad.iter().rev().take_while(|c| **c == b'=').count();
        if pad > 2 || (pad > 0 && !last) {
            return Err(format!("`{}` is not base64: misplaced `=`", shorten(text)));
        }
        let mut n = 0u32;
        for (k, c) in quad.iter().enumerate() {
            let v = if k >= 4 - pad {
                0
            } else {
                match B64.iter().position(|x| x == c) {
                    Some(p) => p as u32,
                    None => {
                        return Err(format!(
                            "`{}` is not base64: `{}` is not in the alphabet",
                            shorten(text),
                            char::from(*c)
                        ));
                    }
                }
            };
            n = n << 6 | v;
        }
        out.extend_from_slice(&n.to_be_bytes()[1..4 - pad]);
    }
    Ok(out)
}

pub fn sha256(data: &[u8]) -> Vec<u8> {
    Sha256::digest(data).to_vec()
}

/// HMAC-SHA256 (RFC 2104): a key longer than the 64-byte block is hashed first.
pub fn hmac_sha256(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut block = [0u8; 64];
    if key.len() > 64 {
        block[..32].copy_from_slice(&sha256(key));
    } else {
        block[..key.len()].copy_from_slice(key);
    }
    let mut inner = Sha256::new();
    inner.update(block.map(|b| b ^ 0x36));
    inner.update(data);
    let mut outer = Sha256::new();
    outer.update(block.map(|b| b ^ 0x5c));
    outer.update(inner.finalize());
    outer.finalize().to_vec()
}

/// The start of a text for an error message.
fn shorten(text: &str) -> String {
    if text.chars().count() > 24 {
        format!("{}…", text.chars().take(23).collect::<String>())
    } else {
        text.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_round_trips_and_names_the_bad_digits() {
        assert_eq!(hex_encode(&[0, 255, 16, 171]), "00ff10ab");
        assert_eq!(hex_decode("00FF10aB").unwrap(), [0, 255, 16, 171]);
        assert_eq!(hex_decode("").unwrap(), Vec::<u8>::new());
        assert!(hex_decode("abc").unwrap_err().contains("even number"));
        let e = hex_decode("00zz").unwrap_err();
        assert!(e.contains("position 2") && e.contains("`zz`"), "{e}");
        let all: Vec<u8> = (0..=255).collect();
        assert_eq!(hex_decode(&hex_encode(&all)).unwrap(), all);
    }

    /// The test vectors of RFC 4648, section 10.
    #[test]
    fn base64_matches_the_rfc_vectors_and_refuses_the_rest() {
        for (plain, coded) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(base64_encode(plain.as_bytes()), coded);
            assert_eq!(base64_decode(coded).unwrap(), plain.as_bytes());
        }
        let all: Vec<u8> = (0..=255).collect();
        assert_eq!(base64_decode(&base64_encode(&all)).unwrap(), all);
        assert_eq!(base64_encode(&[0xfb, 0xff]), "+/8=");
        for bad in ["Zg", "Z===", "Zg=a", "Zm9v\n", "Zm 9", "Zm9=Zm9v", "Zm-v"] {
            assert!(base64_decode(bad).is_err(), "{bad:?} should not decode");
        }
    }

    /// FIPS 180-2 examples.
    #[test]
    fn sha256_matches_the_known_digests() {
        assert_eq!(
            hex_encode(&sha256(b"")),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            hex_encode(&sha256(b"abc")),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            hex_encode(&sha256(
                b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"
            )),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
    }

    /// RFC 4231, test cases 1, 2 and 6 (a key longer than the block).
    #[test]
    fn hmac_sha256_matches_rfc_4231() {
        assert_eq!(
            hex_encode(&hmac_sha256(&[0x0b; 20], b"Hi There")),
            "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7"
        );
        assert_eq!(
            hex_encode(&hmac_sha256(b"Jefe", b"what do ya want for nothing?")),
            "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
        );
        assert_eq!(
            hex_encode(&hmac_sha256(
                &[0xaa; 131],
                b"Test Using Larger Than Block-Size Key - Hash Key First"
            )),
            "60e431591ee0b67f0d8a26aacbf5b77f8e0bc6213728c5140546040f0ee37f54"
        );
    }
}

/* Copyright (c) 2021-2026 Richard Rodger and other contributors, MIT License */

//! Byte-order-mark handling: the port of `decodeBOM` in `ts/src/xml.ts`.

/// Decode a byte sequence into text, transcoding from whichever of UTF-8,
/// UTF-16 LE/BE or UTF-32 LE/BE its byte-order mark indicates. UTF-8 is
/// the default when there is no mark, so BOM-less UTF-8 files with
/// non-ASCII tag names round-trip. The mark itself is dropped.
///
/// Use this when reading XML files of unknown encoding:
///
/// ```no_run
/// let body = tabnas_xml::decode_bom(&std::fs::read("doc.xml")?);
/// let doc = tabnas_xml::parse(&body)?;
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
///
/// A malformed UTF-8 sequence is not an error here: each of its bytes
/// becomes the character with that code, as the canonical decoder does,
/// so a stray control byte is still reported as an illegal XML character
/// by the parser rather than vanishing in transcoding. A unit that is not
/// a Unicode scalar value, in any of the encodings, becomes U+FFFF rather
/// than U+FFFD. U+FFFF is excluded from `Char` and from `NameChar` exactly
/// as a surrogate is, so a document carrying one is rejected here for the
/// reason the other two runtimes reject it; U+FFFD is a legal name
/// character, and folding to it would turn ill-formed documents into
/// well-formed ones. The `not_a_scalar` function in this module carries
/// the full reasoning.
pub fn decode_bom(bytes: &[u8]) -> String {
    match bytes {
        [0x00, 0x00, 0xfe, 0xff, rest @ ..] => decode_utf32(rest, true),
        [0xff, 0xfe, 0x00, 0x00, rest @ ..] => decode_utf32(rest, false),
        [0xfe, 0xff, rest @ ..] => decode_utf16(rest, true),
        [0xff, 0xfe, rest @ ..] => decode_utf16(rest, false),
        [0xef, 0xbb, 0xbf, rest @ ..] => decode_utf8(rest),
        rest => decode_utf8(rest),
    }
}

/// Strip a leading U+FEFF from text that is already decoded: the string
/// arm of the canonical `decodeBOM`.
pub fn strip_bom(src: &str) -> &str {
    src.strip_prefix('\u{FEFF}').unwrap_or(src)
}

/// The stand-in for a code point Rust cannot hold in a `String`: an
/// unpaired surrogate (and, from UTF-32, a value above U+10FFFF).
///
/// TypeScript keeps an unpaired surrogate as it is, because JavaScript
/// strings are UTF-16, and Go keeps the raw bytes, because Go strings are
/// byte sequences. Both then let the XML checks downstream reject it: a
/// surrogate is neither an XML `Char` (2.2 [2]) nor a `NameChar`
/// (2.3 [4a]), so a document carrying one is not well formed, and all
/// three runtimes must say so.
///
/// A Rust `String` holds Unicode scalar values only, so the surrogate
/// cannot survive decoding. Folding it to U+FFFD would be worse than
/// lossy: U+FFFD IS a valid `NameStartChar` (it closes the
/// `[#xFDF0-#xFFFD]` range), so an ill-formed document would become well
/// formed and the port would accept what TypeScript and Go reject. That
/// happened here: eight `eduni/errata-4e` conformance documents with
/// surrogates in element names were accepted until this mapping changed.
///
/// U+FFFF is the faithful substitute. Like a surrogate it is excluded
/// from `Char` and from `NameChar`, so every XML check reaches the same
/// verdict on it that it reaches on the surrogate it replaces. The two
/// are distinguishable only by a caller inspecting the decoded text
/// directly, never by the well-formedness of the document.
///
/// This is narrower than the engine-wide rule in the parser's
/// `DIVERGENCE.md` ("lone surrogates -> U+FFFD"), and deliberately so:
/// that rule governs a character VALUE parsed out of a document, such as
/// `&#xD800;`, where Go and Rust agree on U+FFFD and only the stored
/// value differs. This function governs SOURCE TEXT, where the same fold
/// would change the verdict rather than the value.
fn not_a_scalar(code: u32) -> char {
    debug_assert!(
        char::from_u32(code).is_none(),
        "{code:#x} is a scalar value"
    );
    '\u{FFFF}'
}

fn decode_utf8(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let lead = bytes[i];
        if lead < 0x80 {
            out.push(lead as char);
            i += 1;
            continue;
        }
        let tail = |offset: usize| u32::from(bytes[i + offset] & 0x3f);
        let (code, advance) = if lead & 0xe0 == 0xc0 && i + 1 < bytes.len() {
            ((u32::from(lead & 0x1f) << 6) | tail(1), 2)
        } else if lead & 0xf0 == 0xe0 && i + 2 < bytes.len() {
            ((u32::from(lead & 0x0f) << 12) | (tail(1) << 6) | tail(2), 3)
        } else if lead & 0xf8 == 0xf0 && i + 3 < bytes.len() {
            (
                (u32::from(lead & 0x07) << 18) | (tail(1) << 12) | (tail(2) << 6) | tail(3),
                4,
            )
        } else {
            (u32::MAX, 1)
        };
        // A malformed sequence (an invalid lead byte, a truncated tail, or
        // a code point out of range) emits the raw byte and moves on one
        // position, so the downstream XML check can flag it.
        if code > 0x10ffff {
            out.push(char::from(lead));
            i += 1;
        } else {
            out.push(char::from_u32(code).unwrap_or_else(|| not_a_scalar(code)));
            i += advance;
        }
    }
    out
}

fn decode_utf16(bytes: &[u8], big: bool) -> String {
    let units = bytes.chunks_exact(2).map(|pair| {
        if big {
            u16::from_be_bytes([pair[0], pair[1]])
        } else {
            u16::from_le_bytes([pair[0], pair[1]])
        }
    });
    char::decode_utf16(units)
        .map(|unit| match unit {
            Ok(c) => c,
            Err(unpaired) => not_a_scalar(u32::from(unpaired.unpaired_surrogate())),
        })
        .collect()
}

fn decode_utf32(bytes: &[u8], big: bool) -> String {
    bytes
        .chunks_exact(4)
        .map(|quad| {
            let quad = [quad[0], quad[1], quad[2], quad[3]];
            let code = if big {
                u32::from_be_bytes(quad)
            } else {
                u32::from_le_bytes(quad)
            };
            char::from_u32(code).unwrap_or_else(|| not_a_scalar(code))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_encoding_signature_is_honoured() {
        assert_eq!(decode_bom(b"\xef\xbb\xbf<a/>"), "<a/>");
        assert_eq!(decode_bom(b"<\xc3\xa9/>"), "<é/>");
        assert_eq!(decode_bom(b"\xfe\xff\x00<\x00a\x00/\x00>"), "<a/>");
        assert_eq!(decode_bom(b"\xff\xfe<\x00a\x00/\x00>\x00"), "<a/>");
        assert_eq!(
            decode_bom(b"\x00\x00\xfe\xff\x00\x00\x00<\x00\x00\x00a\x00\x00\x00/\x00\x00\x00>"),
            "<a/>"
        );
        assert_eq!(
            decode_bom(b"\xff\xfe\x00\x00<\x00\x00\x00a\x00\x00\x00/\x00\x00\x00>\x00\x00\x00"),
            "<a/>"
        );
        assert_eq!(decode_bom(b""), "");
    }

    #[test]
    fn malformed_utf8_keeps_the_raw_byte() {
        assert_eq!(decode_bom(b"a\x01\xffb"), "a\u{1}\u{ff}b");
        assert_eq!(decode_bom(b"\xf0\x9f\x98\x80"), "\u{1F600}");
        assert_eq!(decode_bom(b"\xe2\x82"), "\u{e2}\u{82}");
    }

    #[test]
    fn an_unpaired_surrogate_stays_invalid() {
        // U+D800 as CESU-8 style bytes, as eight `eduni/errata-4e`
        // conformance documents spell it. The decoded character must not
        // be a NameStartChar, or those documents parse as well formed.
        assert_eq!(decode_bom(b"\xed\xa0\x80"), "\u{FFFF}");
        assert!(!crate::entity::is_name_start('\u{FFFF}'));
        assert!(!crate::entity::is_name_char('\u{FFFF}'));
        // U+FFFD would be, which is why it is not used here.
        assert!(crate::entity::is_name_start('\u{FFFD}'));

        // The same in UTF-16 (an unpaired high surrogate) and UTF-32.
        assert_eq!(decode_bom(b"\xfe\xff\xd8\x00"), "\u{FFFF}");
        assert_eq!(decode_bom(b"\x00\x00\xfe\xff\x00\x00\xd8\x00"), "\u{FFFF}");

        // A surrogate PAIR is a scalar value and is unaffected.
        assert_eq!(decode_bom(b"\xfe\xff\xd8\x3d\xde\x00"), "\u{1F600}");
    }

    #[test]
    fn a_decoded_string_only_loses_its_mark() {
        assert_eq!(strip_bom("\u{FEFF}<a/>"), "<a/>");
        assert_eq!(strip_bom("<a>\u{FEFF}</a>"), "<a>\u{FEFF}</a>");
    }
}

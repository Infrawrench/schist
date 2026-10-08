//! An object's bytes as tagged chunks.
//!
//! Every object in the specimens, except raw image data and the XMP
//! packet, is a run of chunks: a little-endian 32-bit tag, a 32-bit
//! length and that many bytes. The tags are stable across the three
//! specimens and two InDesign versions; what each holds is recovered one
//! tag at a time against the paired IDML.

use crate::database::{u16_at, u32_at};

/// The chunks of an object, stopping at the first that overruns it.
pub fn chunks(bytes: &[u8]) -> impl Iterator<Item = (u32, &[u8])> {
    let mut at = 0usize;
    std::iter::from_fn(move || {
        let tag = u32_at(bytes, at)?;
        let length = u32_at(bytes, at + 4)? as usize;
        let body = bytes.get(at + 8..(at + 8).checked_add(length)?)?;
        at += 8 + length;
        Some((tag, body))
    })
}

/// The first chunk with `tag`.
pub fn find(bytes: &[u8], tag: u32) -> Option<&[u8]> {
    chunks(bytes).find(|(t, _)| *t == tag).map(|(_, body)| body)
}

/// Sequential little-endian reads over a chunk.
pub struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    pub fn new(bytes: &'a [u8]) -> Reader<'a> {
        Reader { bytes, at: 0 }
    }

    pub fn at(bytes: &'a [u8], at: usize) -> Reader<'a> {
        Reader { bytes, at }
    }

    pub fn position(&self) -> usize {
        self.at
    }

    pub fn u8(&mut self) -> Option<u8> {
        let value = *self.bytes.get(self.at)?;
        self.at += 1;
        Some(value)
    }

    pub fn u16(&mut self) -> Option<u16> {
        let value = u16_at(self.bytes, self.at)?;
        self.at += 2;
        Some(value)
    }

    pub fn u32(&mut self) -> Option<u32> {
        let value = u32_at(self.bytes, self.at)?;
        self.at += 4;
        Some(value)
    }

    pub fn f64(&mut self) -> Option<f64> {
        let bytes = self.take(8)?;
        Some(f64::from_le_bytes(bytes.try_into().ok()?))
    }

    pub fn take(&mut self, count: usize) -> Option<&'a [u8]> {
        let bytes = self.bytes.get(self.at..self.at.checked_add(count)?)?;
        self.at += count;
        Some(bytes)
    }

    /// `count` doubles.
    pub fn f64s<const N: usize>(&mut self) -> Option<[f64; N]> {
        let mut out = [0.0; N];
        for value in &mut out {
            *value = self.f64()?;
        }
        Some(out)
    }

    /// One text segment: a head word whose low fourteen bits count what
    /// follows, with 0x4000 for single bytes and 0x8000 for UTF-16 code
    /// units. The specimens' single-byte segments are all ASCII; anything
    /// else they hold is in UTF-16 segments.
    pub fn segment(&mut self) -> Option<(String, usize)> {
        let head = self.u16()?;
        let count = (head & 0x3fff) as usize;
        match head & 0xc000 {
            0x4000 => Some((
                self.take(count)?.iter().map(|&b| b as char).collect(),
                count,
            )),
            0x8000 => {
                let units: Vec<u16> = self
                    .take(count * 2)?
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|pair| u16::from_le_bytes(*pair))
                    .collect();
                Some((String::from_utf16_lossy(&units), count))
            }
            _ => None,
        }
    }

    /// A string field: a flag byte that belongs to the object rather than
    /// the string, a word of 2, the length in characters and then
    /// segments until that many characters are read.
    pub fn string(&mut self) -> Option<String> {
        self.u8()?;
        self.u16().filter(|kind| *kind == 2)?;
        let mut remaining = self.u16()? as usize;
        let mut out = String::new();
        while remaining > 0 {
            let (text, count) = self.segment()?;
            remaining = remaining.checked_sub(count)?;
            out.push_str(&text);
        }
        Some(out)
    }

    /// A count followed by that many UIDs.
    pub fn uids(&mut self) -> Option<Vec<u32>> {
        let count = self.u32()? as usize;
        if count > self.bytes.len() / 4 {
            return None;
        }
        (0..count).map(|_| self.u32()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunks_stop_at_an_overrun_rather_than_reading_past_it() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&0x304u32.to_le_bytes());
        bytes.extend_from_slice(&2u32.to_le_bytes());
        bytes.extend_from_slice(&[7, 8]);
        bytes.extend_from_slice(&0x305u32.to_le_bytes());
        bytes.extend_from_slice(&100u32.to_le_bytes());
        bytes.extend_from_slice(&[1, 2, 3]);
        let found: Vec<_> = chunks(&bytes).collect();
        assert_eq!(found, vec![(0x304, &[7u8, 8][..])]);
    }

    #[test]
    fn a_string_reads_byte_and_utf16_segments() {
        // "Layer 1" as the specimens store a layer name, then the same
        // with an UTF-16 segment for a character outside ASCII.
        let ascii = [
            2, 2, 0, 7, 0, 0x07, 0x40, b'L', b'a', b'y', b'e', b'r', b' ', b'1',
        ];
        assert_eq!(Reader::new(&ascii).string().as_deref(), Some("Layer 1"));
        let mixed = [
            0, 2, 0, 3, 0, 0x02, 0x40, b'C', b'a', 0x01, 0x80, 0xe9, 0x00,
        ];
        assert_eq!(Reader::new(&mixed).string().as_deref(), Some("Caé"));
    }

    #[test]
    fn an_unknown_segment_encoding_is_refused() {
        let bytes = [0, 2, 0, 1, 0, 0x01, 0x00, b'x'];
        assert_eq!(Reader::new(&bytes).string(), None);
    }
}

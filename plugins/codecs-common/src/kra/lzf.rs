//! LZF, the byte-oriented LZ77 variant Krita compresses tiles with.
//!
//! A control byte below 32 introduces `ctrl + 1` literal bytes. Any other
//! control byte is a back reference: its top three bits are the length
//! minus two (seven meaning "add the next byte"), its low five bits and the
//! byte after the length are the distance minus one.

/// Decompress `input`, which must produce exactly `len` bytes.
///
/// Like Krita's reader, a single trailing byte that cannot start a token
/// is ignored. Anything that would read or write out of bounds, or
/// reference before the start of the output, is `None`.
pub fn decompress(input: &[u8], len: usize) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(len);
    let mut i = 0;
    while i + 1 < input.len() {
        let ctrl = input[i] as usize;
        i += 1;
        if ctrl < 32 {
            let n = ctrl + 1;
            if out.len() + n > len {
                return None;
            }
            out.extend_from_slice(input.get(i..i + n)?);
            i += n;
        } else {
            let mut n = ctrl >> 5;
            if n == 7 {
                n += *input.get(i)? as usize;
                i += 1;
            }
            n += 2;
            let back = ((ctrl & 31) << 8) + *input.get(i)? as usize + 1;
            i += 1;
            if back > out.len() || out.len() + n > len {
                return None;
            }
            // Byte by byte: the source may overlap what is being written.
            let start = out.len() - back;
            for k in 0..n {
                let b = out[start + k];
                out.push(b);
            }
        }
    }
    (out.len() == len).then_some(out)
}

/// A straightforward LZF compressor, used to build test tiles. Output is
/// readable by `decompress` and by Krita.
#[cfg(test)]
pub fn compress(input: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut literals: Vec<u8> = Vec::new();
    let flush = |out: &mut Vec<u8>, literals: &mut Vec<u8>| {
        for chunk in literals.chunks(32) {
            out.push(chunk.len() as u8 - 1);
            out.extend_from_slice(chunk);
        }
        literals.clear();
    };
    let mut table = std::collections::HashMap::<[u8; 3], usize>::new();
    let mut i = 0;
    while i < input.len() {
        let mut matched = None;
        if i + 3 <= input.len() {
            let key = [input[i], input[i + 1], input[i + 2]];
            if let Some(&at) = table.get(&key) {
                if i - at <= 8192 {
                    let mut n = 0;
                    while i + n < input.len() && n < 264 && input[at + n] == input[i + n] {
                        n += 1;
                    }
                    if n >= 3 {
                        matched = Some((at, n));
                    }
                }
            }
            table.insert(key, i);
        }
        match matched {
            Some((at, n)) => {
                flush(&mut out, &mut literals);
                let back = i - at - 1;
                let len = n - 2;
                if len < 7 {
                    out.push(((len << 5) | (back >> 8)) as u8);
                } else {
                    out.push(((7 << 5) | (back >> 8)) as u8);
                    out.push((len - 7) as u8);
                }
                out.push(back as u8);
                i += n;
            }
            None => {
                literals.push(input[i]);
                i += 1;
            }
        }
    }
    flush(&mut out, &mut literals);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literals_and_overlapping_back_references() {
        // "ab" literally, then a reference 2 back for 6 bytes.
        let stream = [1, b'a', b'b', (4 << 5), 1];
        assert_eq!(decompress(&stream, 8).unwrap(), b"abababab");
        // The long form: length byte follows the control byte.
        let stream = [0, b'x', (7 << 5), 3, 0];
        assert_eq!(decompress(&stream, 13).unwrap(), vec![b'x'; 13]);
    }

    #[test]
    fn hostile_streams_fail_cleanly() {
        for (stream, len) in [
            (&[5u8, 1, 2][..], 6),         // literal runs past the input
            (&[0, 1, (1 << 5), 5][..], 4), // reference before the start
            (&[0, 1, (6 << 5), 0][..], 3), // output overflow
            (&[0, 1][..], 2),              // too short a result
            (&[][..], 1),
        ] {
            assert!(decompress(stream, len).is_none(), "{stream:?}");
        }
    }

    #[test]
    fn compressor_round_trips() {
        let mut data: Vec<u8> = (0..20_000u32).map(|i| (i * 7 % 13) as u8).collect();
        data.extend((0..5000u32).map(|i| (i.wrapping_mul(2654435761) >> 24) as u8));
        data.extend(std::iter::repeat_n(9, 1000));
        let packed = compress(&data);
        assert!(packed.len() < data.len());
        assert_eq!(decompress(&packed, data.len()).unwrap(), data);
    }
}

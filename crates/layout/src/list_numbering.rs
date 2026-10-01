//! Counter representations shared by composition and the Paragraph controls.
use crate::lists::NumberingFormat;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CounterFormat {
    Decimal,
    UpperRoman,
    LowerRoman,
    UpperLetters,
    LowerLetters,
    SingleLeadingZeros,
    DoubleLeadingZeros,
    TripleLeadingZeros,
    None,
}
impl CounterFormat {
    pub const ALL: [Self; 9] = [
        Self::Decimal,
        Self::UpperRoman,
        Self::LowerRoman,
        Self::UpperLetters,
        Self::LowerLetters,
        Self::SingleLeadingZeros,
        Self::DoubleLeadingZeros,
        Self::TripleLeadingZeros,
        Self::None,
    ];
    pub fn native(self) -> NumberingFormat {
        NumberingFormat::Enumeration {
            enumeration: match self {
                Self::Decimal => "Arabic",
                Self::UpperRoman => "UpperRoman",
                Self::LowerRoman => "LowerRoman",
                Self::UpperLetters => "UpperLetters",
                Self::LowerLetters => "LowerLetters",
                Self::SingleLeadingZeros => "SingleLeadingZeros",
                Self::DoubleLeadingZeros => "DoubleLeadingZeros",
                Self::TripleLeadingZeros => "TripleLeadingZeros",
                Self::None => "FormatNone",
            }
            .into(),
        }
    }

    /// Roman numbering is supported in its conventional 1–3999 range. Larger
    /// native values are retained and diagnosed rather than given invented
    /// overbar/repeated-M semantics. Alphabetic counters use bijective base 26,
    /// unlike repeated-letter PDF page labels. Padding never truncates digits.
    pub fn render(self, number: u64) -> Result<String, &'static str> {
        if number == 0 {
            return Err("NumberingStartAt");
        }
        Ok(match self {
            Self::None => String::new(),
            Self::Decimal => number.to_string(),
            Self::SingleLeadingZeros => format!("{number:02}"),
            Self::DoubleLeadingZeros => format!("{number:03}"),
            Self::TripleLeadingZeros => format!("{number:04}"),
            Self::UpperRoman | Self::LowerRoman => {
                if number > 3999 {
                    return Err("NumberingFormat.RomanRange");
                }
                let style = if self == Self::UpperRoman {
                    crate::geometry::NumberStyle::RomanUpper
                } else {
                    crate::geometry::NumberStyle::RomanLower
                };
                style.format(number as u32)
            }
            Self::UpperLetters | Self::LowerLetters => {
                let first = if self == Self::UpperLetters {
                    b'A'
                } else {
                    b'a'
                };
                let mut number = number;
                let mut letters = Vec::new();
                while number > 0 {
                    number -= 1;
                    letters.push(char::from(first + (number % 26) as u8));
                    number /= 26;
                }
                letters.into_iter().rev().collect()
            }
        })
    }
}

impl NumberingFormat {
    /// Recognize the native enumeration names and conventional named formats.
    /// The original value/type is always retained, including whitespace.
    pub fn counter_format(&self) -> Option<CounterFormat> {
        use CounterFormat::*;
        let value: String = self
            .value()
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect();
        Some(match value.trim_end_matches(['.', '…']) {
            "Arabic" | "1,2,3" | "1,2,3,4" => Decimal,
            "UpperRoman" | "I,II,III" | "I,II,III,IV" => UpperRoman,
            "LowerRoman" | "i,ii,iii" | "i,ii,iii,iv" => LowerRoman,
            "UpperLetters" | "A,B,C" | "A,B,C,D" => UpperLetters,
            "LowerLetters" | "a,b,c" | "a,b,c,d" => LowerLetters,
            "SingleLeadingZeros" | "01,02,03" | "01,02,03,04" => SingleLeadingZeros,
            "DoubleLeadingZeros" | "001,002,003" | "001,002,003,004" => DoubleLeadingZeros,
            "TripleLeadingZeros" | "0001,0002,0003" | "0001,0002,0003,0004" => TripleLeadingZeros,
            "FormatNone" => CounterFormat::None,
            _ => return Option::None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counter_representations_are_reversible_across_every_digit_or_letter_boundary() {
        for number in (1..=20_000u64).chain([u32::MAX as u64, u64::MAX]) {
            for (format, width) in [
                (CounterFormat::Decimal, 1),
                (CounterFormat::SingleLeadingZeros, 2),
                (CounterFormat::DoubleLeadingZeros, 3),
                (CounterFormat::TripleLeadingZeros, 4),
            ] {
                let text = format.render(number).unwrap();
                assert_eq!(text.parse::<u64>().unwrap(), number);
                assert_eq!(text.len(), number.to_string().len().max(width));
            }
            for format in [CounterFormat::UpperLetters, CounterFormat::LowerLetters] {
                let text = format.render(number).unwrap();
                assert!(text.len() <= 14);
                let decoded = text.bytes().fold(0u128, |value, c| {
                    value * 26 + u128::from(c.to_ascii_uppercase() - b'A' + 1)
                });
                assert_eq!(decoded, u128::from(number));
                assert_eq!(
                    text,
                    if format == CounterFormat::UpperLetters {
                        text.to_uppercase()
                    } else {
                        text.to_lowercase()
                    }
                );
            }
        }
        for number in 1..=3999 {
            let upper = CounterFormat::UpperRoman.render(number).unwrap();
            assert_eq!(
                CounterFormat::LowerRoman.render(number).unwrap(),
                upper.to_lowercase()
            );
            let mut total = 0i64;
            let mut right = 0;
            for c in upper.chars().rev() {
                let value = match c {
                    'M' => 1000,
                    'D' => 500,
                    'C' => 100,
                    'L' => 50,
                    'X' => 10,
                    'V' => 5,
                    'I' => 1,
                    _ => panic!("invalid numeral"),
                };
                total += if value < right { -value } else { value };
                right = right.max(value);
            }
            assert_eq!(total as u64, number);
        }
        for number in [4000, u32::MAX as u64, u64::MAX] {
            assert_eq!(
                CounterFormat::UpperRoman.render(number),
                Err("NumberingFormat.RomanRange")
            );
        }
        for format in CounterFormat::ALL {
            assert_eq!(format.native().counter_format(), Some(format));
            assert_eq!(format.render(0), Err("NumberingStartAt"));
        }
        assert_eq!(CounterFormat::None.render(u64::MAX), Ok(String::new()));
    }
}

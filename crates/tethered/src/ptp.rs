//! Small, bounded PTP dataset helpers shared by native transports.
use crate::Result;
use schist_i18n::t;

fn invalid() -> String {
    t("tethered.no_download").into()
}

pub fn command(operation: u16, transaction: u32, parameters: &[u32]) -> Vec<u8> {
    assert!(parameters.len() <= 5);
    let mut bytes = Vec::with_capacity(12 + parameters.len() * 4);
    bytes.extend_from_slice(&(12u32 + parameters.len() as u32 * 4).to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&operation.to_le_bytes());
    bytes.extend_from_slice(&transaction.to_le_bytes());
    for parameter in parameters {
        bytes.extend_from_slice(&parameter.to_le_bytes());
    }
    bytes
}

pub fn response_ok(bytes: &[u8]) -> bool {
    (12..=32).contains(&bytes.len())
        && (bytes.len() - 12).is_multiple_of(4)
        && u32::from_le_bytes(bytes[..4].try_into().unwrap()) as usize == bytes.len()
        && bytes[4..6] == 3u16.to_le_bytes()
        && bytes[6..8] == 0x2001u16.to_le_bytes()
}

struct Dataset<'a> {
    bytes: &'a [u8],
    offset: usize,
}
impl<'a> Dataset<'a> {
    fn take(&mut self, count: usize) -> Result<&'a [u8]> {
        let end = self.offset.checked_add(count).ok_or_else(invalid)?;
        let result = self.bytes.get(self.offset..end).ok_or_else(invalid)?;
        self.offset = end;
        Ok(result)
    }
    fn string(&mut self) -> Result<String> {
        let count = self.take(1)?[0] as usize;
        let data = self.take(count * 2)?;
        let mut chars: Vec<_> = data
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        if count > 0 && chars.pop() != Some(0) {
            return Err(invalid());
        }
        String::from_utf16(&chars).map_err(|_| invalid())
    }
}

/// DeviceInfo payload (without the USB data-container header).
pub fn supports_capture(bytes: &[u8]) -> Result<bool> {
    let mut data = Dataset { bytes, offset: 0 };
    data.take(8)?; // standard version, vendor extension ID and version
    data.string()?;
    data.take(2)?; // functional mode
    let count = u32::from_le_bytes(data.take(4)?.try_into().unwrap()) as usize;
    let operations = data.take(count.checked_mul(2).ok_or_else(invalid)?)?;
    Ok(operations
        .as_chunks::<2>()
        .0
        .iter()
        .any(|op| u16::from_le_bytes(*op) == 0x100e))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn capture_requires_advertised_operation_and_complete_dataset() {
        let mut info = vec![0; 8];
        info.extend_from_slice(&[0, 0, 0]);
        info.extend_from_slice(&2u32.to_le_bytes());
        info.extend_from_slice(&0x1001u16.to_le_bytes());
        info.extend_from_slice(&0x100eu16.to_le_bytes());
        assert!(supports_capture(&info).unwrap());
        for end in 0..info.len() {
            assert!(supports_capture(&info[..end]).is_err());
        }
        *info.last_mut().unwrap() = 0;
        assert!(!supports_capture(&info).unwrap());
    }
    #[test]
    fn wire_commands_and_responses_are_bounded() {
        let command = command(0x100e, 7, &[0, 0]);
        assert_eq!(
            command,
            [20, 0, 0, 0, 1, 0, 14, 16, 7, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]
        );
        assert!(!response_ok(&command));
        assert!(response_ok(&[12, 0, 0, 0, 3, 0, 1, 32, 7, 0, 0, 0]));
        assert!(!response_ok(&[12, 0, 0, 0, 3, 0, 2, 32, 7, 0, 0, 0]));
    }
}

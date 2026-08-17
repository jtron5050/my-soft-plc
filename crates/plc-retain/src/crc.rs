//! IEEE CRC-32 (ISO 3309 / PKZIP polynomial).

const POLY: u32 = 0xEDB8_8320;

/// CRC-32 of `data` (init `0xFFFF_FFFF`, final invert).
#[must_use]
pub fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc ^= u32::from(b);
        for _ in 0..8 {
            crc = if crc & 1 == 0 {
                crc >> 1
            } else {
                (crc >> 1) ^ POLY
            };
        }
    }
    !crc
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc32_empty_and_known() {
        // ISO-HDLC / PKZIP empty input.
        assert_eq!(crc32(b""), 0);
        // "123456789" is the conventional check vector 0xCBF43926.
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }
}

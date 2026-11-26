// SPDX-License-Identifier: GPL-3.0-or-later
//! The SIP2 checksum (spec item 5).

/// The checksum of `bytes`: the two's complement, in 16 bits, of their sum.
///
/// Koha: `C4::SIP::Sip::Checksum::checksum`.
#[must_use]
pub fn checksum(bytes: &[u8]) -> u16 {
    let sum = bytes
        .iter()
        .fold(0_u16, |sum, &byte| sum.wrapping_add(u16::from(byte)));
    sum.wrapping_neg()
}

/// Whether `message` ends in `AZ` and four hexadecimal digits that are the checksum of
/// everything before them.
///
/// Koha: `C4::SIP::Sip::Checksum::verify_cksum`.
#[must_use]
pub fn verify_checksum(message: &[u8]) -> bool {
    let Some((body, digits)) = message.split_last_chunk::<4>() else {
        return false;
    };
    if !body.ends_with(b"AZ") || !digits.iter().all(u8::is_ascii_hexdigit) {
        return false;
    }
    let Ok(digits) = std::str::from_utf8(digits) else {
        return false;
    };
    u16::from_str_radix(digits, 16).is_ok_and(|sum| sum == checksum(body))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_checksums_of_kohas_tests() {
        assert_eq!(checksum(b"12345"), 65281);
        assert_eq!(checksum(b"teststring"), 64425);
        let sum = checksum(b"abcdAZ");
        assert!(verify_checksum(format!("abcdAZ{sum:04X}").as_bytes()));
        assert!(!verify_checksum(b"1234567"));
        assert!(!verify_checksum(b"abcdAZ0000"));
        assert!(verify_checksum(b"97AZFEF5"));
        assert!(verify_checksum(b"96AZFEF6"));
    }
}

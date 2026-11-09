// SPDX-License-Identifier: GPL-3.0-or-later
//! Battery levels from the Bluetooth LE "proximity pairing" broadcast that AirPods and
//! Beats send (Apple manufacturer data, type 0x07). The format is not published; this follows
//! what community projects (OpenPods, AirPodsDesktop) have reverse-engineered.

/// Apple's Bluetooth company id.
pub const APPLE_COMPANY_ID: u16 = 0x004C;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pods {
    pub left: Option<u8>,
    pub right: Option<u8>,
    pub case: Option<u8>,
    /// Model code as sent over the air (e.g. 0x2019).
    pub model: u16,
}

/// One battery nibble: 0..=9 is "n5 %" (5, 15, ... 95), 10 is 100 %, anything else unknown.
fn level(nibble: u8) -> Option<u8> {
    match nibble {
        0..=9 => Some(nibble * 10 + 5),
        10 => Some(100),
        _ => None,
    }
}

/// `data` is the manufacturer data after the company id. `None` when it is not a
/// proximity-pairing message.
pub fn parse_proximity(data: &[u8]) -> Option<Pods> {
    if data.len() < 8 || data[0] != 0x07 {
        return None;
    }
    let (high, low) = (data[6] >> 4, data[6] & 0x0F);
    // Bit 0x20 of the status byte says which nibble belongs to the left bud.
    let (left, right) = if data[5] & 0x20 == 0 { (high, low) } else { (low, high) };
    Some(Pods { left: level(left), right: level(right), case: level(data[7] & 0x0F), model: u16::from_be_bytes([data[3], data[4]]) })
}

impl Pods {
    /// The number to show: the lower of the two buds when both report, else whichever
    /// does, else the case.
    pub fn display_percent(&self) -> Option<u8> {
        match (self.left, self.right) {
            (Some(l), Some(r)) => Some(l.min(r)),
            (Some(x), None) | (None, Some(x)) => Some(x),
            (None, None) => self.case,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// type, length, 0x01, model 0x2019, status, pods, flags/case, then filler.
    fn msg(status: u8, pods: u8, flags_case: u8) -> Vec<u8> {
        let mut v = vec![0x07, 0x19, 0x01, 0x20, 0x19, status, pods, flags_case];
        v.resize(27, 0);
        v
    }

    #[test]
    fn levels_map_nibbles_to_percent() {
        assert_eq!((level(0), level(8), level(9), level(10), level(15)), (Some(5), Some(85), Some(95), Some(100), None));
    }

    #[test]
    fn reads_left_right_and_case() {
        // Status bit 0x20 set: the low nibble is the left bud.
        let p = parse_proximity(&msg(0x20, 0x8A, 0x05)).unwrap();
        assert_eq!(p.model, 0x2019);
        assert_eq!((p.left, p.right, p.case), (Some(100), Some(85), Some(55)));
    }

    #[test]
    fn the_buds_swap_when_the_flip_bit_is_clear() {
        let p = parse_proximity(&msg(0x00, 0x8A, 0x05)).unwrap();
        assert_eq!((p.left, p.right), (Some(85), Some(100)));
    }

    #[test]
    fn unknown_nibbles_are_none() {
        let p = parse_proximity(&msg(0x20, 0xF8, 0x0F)).unwrap();
        assert_eq!((p.left, p.right, p.case), (Some(85), None, None));
    }

    #[test]
    fn other_apple_messages_and_short_data_are_ignored() {
        assert_eq!(parse_proximity(&[0x10, 0x05, 0x01]), None);
        assert_eq!(parse_proximity(&[0x07, 0x19, 0x01]), None);
        assert_eq!(parse_proximity(&[]), None);
    }

    #[test]
    fn display_percent_prefers_the_lower_bud_then_the_case() {
        let p = |l, r, c| Pods { left: l, right: r, case: c, model: 0 };
        assert_eq!(p(Some(80), Some(60), Some(100)).display_percent(), Some(60));
        assert_eq!(p(Some(80), None, Some(100)).display_percent(), Some(80));
        assert_eq!(p(None, None, Some(40)).display_percent(), Some(40));
        assert_eq!(p(None, None, None).display_percent(), None);
    }
}

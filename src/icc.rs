//! An sRGB ICC profile, made in code so that there is no file whose licence matters. It is a
//! version 2 matrix/TRC display profile with the primaries of sRGB (IEC 61966-2-1) adapted to
//! the D50 illuminant of the profile connection space, and the transfer curve of sRGB sampled at
//! 1024 points. Its bytes do not depend on anything, in particular not on the time.

/// A number in the s15Fixed16 format of ICC profiles.
fn s15_fixed16(value: f64) -> [u8; 4] {
    ((value * 65536.0).round() as i32).to_be_bytes()
}

fn xyz_tag(x: f64, y: f64, z: f64) -> Vec<u8> {
    let mut tag = b"XYZ \0\0\0\0".to_vec();
    for value in [x, y, z] {
        tag.extend(s15_fixed16(value));
    }
    tag
}

fn text_tag(text: &str) -> Vec<u8> {
    let mut tag = b"text\0\0\0\0".to_vec();
    tag.extend(text.as_bytes());
    tag.push(0);
    tag
}

/// `textDescriptionType` of ICC version 2.
fn description_tag(text: &str) -> Vec<u8> {
    let mut tag = b"desc\0\0\0\0".to_vec();
    tag.extend((text.len() as u32 + 1).to_be_bytes());
    tag.extend(text.as_bytes());
    tag.push(0);
    // no Unicode and no Macintosh description
    tag.extend([0u8; 4 + 4 + 2 + 1 + 67]);
    tag
}

/// The transfer curve of sRGB (its inverse EOTF) as a table of 1024 entries.
fn srgb_curve() -> Vec<u8> {
    const POINTS: usize = 1024;
    let mut tag = b"curv\0\0\0\0".to_vec();
    tag.extend((POINTS as u32).to_be_bytes());
    for index in 0..POINTS {
        let v = index as f64 / (POINTS - 1) as f64;
        let linear = if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) };
        tag.extend(((linear * 65535.0).round() as u16).to_be_bytes());
    }
    tag
}

/// An sRGB profile (ICC.1:2001-04, version 2.1) for the output intent of a PDF/A file.
pub fn srgb_profile() -> Vec<u8> {
    // the colorants of sRGB adapted to D50, and D50 itself
    let (red, green, blue) = ((0.43607, 0.22249, 0.01392), (0.38515, 0.71687, 0.09708), (0.14307, 0.06061, 0.71410));
    const D50: (f64, f64, f64) = (0.9642, 1.0, 0.8249);
    let curve = srgb_curve();
    // the signatures of the tags and their data (the three curves share theirs)
    let mut data: Vec<([u8; 4], Vec<u8>)> = vec![
        (*b"desc", description_tag("sRGB IEC61966-2.1")),
        (*b"cprt", text_tag("Public Domain")),
        (*b"wtpt", xyz_tag(D50.0, D50.1, D50.2)),
        (*b"rXYZ", xyz_tag(red.0, red.1, red.2)),
        (*b"gXYZ", xyz_tag(green.0, green.1, green.2)),
        (*b"bXYZ", xyz_tag(blue.0, blue.1, blue.2)),
        (*b"rTRC", curve),
    ];
    let tag_count = data.len() + 2;
    let mut offset = 128 + 4 + 12 * tag_count;
    let mut table = Vec::new();
    let mut body = Vec::new();
    for (signature, bytes) in data.drain(..) {
        let size = bytes.len();
        table.extend(signature);
        table.extend((offset as u32).to_be_bytes());
        table.extend((size as u32).to_be_bytes());
        if signature == *b"rTRC" {
            // the curve of green and blue is the one of red
            for other in [*b"gTRC", *b"bTRC"] {
                table.extend(other);
                table.extend((offset as u32).to_be_bytes());
                table.extend((size as u32).to_be_bytes());
            }
        }
        body.extend(bytes);
        // the data of every tag starts at a multiple of 4 bytes
        let padding = (4 - body.len() % 4) % 4;
        body.extend(std::iter::repeat(0u8).take(padding));
        offset = 128 + 4 + 12 * tag_count + body.len();
    }
    debug_assert_eq!(table.len(), 12 * tag_count);

    let mut profile = Vec::new();
    profile.extend(((128 + 4 + table.len() + body.len()) as u32).to_be_bytes());
    profile.extend([0u8; 4]); // no preferred CMM
    profile.extend(0x0210_0000u32.to_be_bytes()); // version 2.1.0
    profile.extend(*b"mntr");
    profile.extend(*b"RGB ");
    profile.extend(*b"XYZ ");
    for field in [2024u16, 1, 1, 0, 0, 0] {
        profile.extend(field.to_be_bytes()); // a fixed date
    }
    profile.extend(*b"acsp");
    profile.extend([0u8; 4 + 4 + 4 + 4 + 8]); // platform, flags, manufacturer, model, attributes
    profile.extend([0u8; 4]); // the perceptual rendering intent
    for value in [D50.0, D50.1, D50.2] {
        profile.extend(s15_fixed16(value));
    }
    profile.extend([0u8; 4]); // creator
    profile.resize(128, 0); // profile ID (not set) and the reserved bytes
    profile.extend((tag_count as u32).to_be_bytes());
    profile.extend(table);
    profile.extend(body);
    profile
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::convert::TryInto;

    #[test]
    fn the_profile_is_well_formed() {
        let profile = srgb_profile();
        let u32_at = |at: usize| u32::from_be_bytes(profile[at..at + 4].try_into().unwrap()) as usize;
        assert_eq!(u32_at(0), profile.len());
        assert_eq!(&profile[36..40], b"acsp");
        assert_eq!(&profile[12..24], b"mntrRGB XYZ ");
        assert_eq!(profile.len() % 4, 0);
        // every tag is inside the profile, at a multiple of 4 bytes
        let tags = u32_at(128);
        assert_eq!(tags, 9);
        for tag in 0..tags {
            let (offset, size) = (u32_at(132 + 12 * tag + 4), u32_at(132 + 12 * tag + 8));
            assert_eq!(offset % 4, 0);
            assert!(offset + size <= profile.len());
        }
        // the curve is monotonic and reaches white
        let (offset, size) = (u32_at(132 + 12 * 6 + 4), u32_at(132 + 12 * 6 + 8));
        assert_eq!(size, 12 + 2 * 1024);
        let values: Vec<u16> = profile[offset + 12..offset + size]
            .chunks(2)
            .map(|pair| u16::from_be_bytes([pair[0], pair[1]]))
            .collect();
        assert!(values.windows(2).all(|pair| pair[0] <= pair[1]));
        assert_eq!((values[0], values[1023]), (0, 65535));
    }

    #[test]
    fn the_profile_does_not_change() {
        assert_eq!(srgb_profile(), srgb_profile());
    }
}

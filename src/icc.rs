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

/// One tag of a profile. The tags in `aliases` have the same data (stored once), like the three
/// curves of a profile with one curve for every channel.
struct Tag {
    signature: [u8; 4],
    data: Vec<u8>,
    aliases: Vec<[u8; 4]>,
}

impl Tag {
    fn new(signature: &[u8; 4], data: Vec<u8>) -> Self {
        Tag { signature: *signature, data, aliases: Vec::new() }
    }

    fn shared(signature: &[u8; 4], aliases: &[&[u8; 4]], data: Vec<u8>) -> Self {
        Tag { signature: *signature, data, aliases: aliases.iter().map(|alias| **alias).collect() }
    }
}

/// The tristimulus value of the white of the profile connection space (D50).
const D50: (f64, f64, f64) = (0.9642, 1.0, 0.8249);

/// Puts the header, the tag table and the data of the tags together: an RGB display profile with
/// the profile connection space XYZ, of ICC version `version` (like `0x0210_0000`). The bytes do
/// not depend on anything, in particular not on the time.
fn assemble(version: u32, tags: Vec<Tag>) -> Vec<u8> {
    let tag_count: usize = tags.iter().map(|tag| 1 + tag.aliases.len()).sum();
    let first_data = 128 + 4 + 12 * tag_count;
    let mut table = Vec::new();
    let mut body = Vec::new();
    for tag in tags {
        let offset = first_data + body.len();
        for signature in std::iter::once(tag.signature).chain(tag.aliases.iter().copied()) {
            table.extend(signature);
            table.extend((offset as u32).to_be_bytes());
            table.extend((tag.data.len() as u32).to_be_bytes());
        }
        body.extend(&tag.data);
        // the data of every tag starts at a multiple of 4 bytes
        let padding = (4 - body.len() % 4) % 4;
        body.extend(std::iter::repeat(0u8).take(padding));
    }
    debug_assert_eq!(table.len(), 12 * tag_count);

    let mut profile = Vec::new();
    profile.extend(((first_data + body.len()) as u32).to_be_bytes());
    profile.extend([0u8; 4]); // no preferred CMM
    profile.extend(version.to_be_bytes());
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

/// An sRGB profile (ICC.1:2001-04, version 2.1) for the output intent of a PDF/A file.
pub fn srgb_profile() -> Vec<u8> {
    // the colorants of sRGB adapted to D50
    let (red, green, blue) = ((0.43607, 0.22249, 0.01392), (0.38515, 0.71687, 0.09708), (0.14307, 0.06061, 0.71410));
    assemble(
        0x0210_0000,
        vec![
            Tag::new(b"desc", description_tag("sRGB IEC61966-2.1")),
            Tag::new(b"cprt", text_tag("Public Domain")),
            Tag::new(b"wtpt", xyz_tag(D50.0, D50.1, D50.2)),
            Tag::new(b"rXYZ", xyz_tag(red.0, red.1, red.2)),
            Tag::new(b"gXYZ", xyz_tag(green.0, green.1, green.2)),
            Tag::new(b"bXYZ", xyz_tag(blue.0, blue.1, blue.2)),
            // the curve of green and blue is the one of red
            Tag::shared(b"rTRC", &[b"gTRC", b"bTRC"], srgb_curve()),
        ],
    )
}

/// A transfer curve of the `parametricCurveType` of ICC version 4, function type 3: with the
/// encoded value `x` the linear value is `(a x + b)^g` for `x >= d` and `c x` below.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ParametricCurve {
    pub g: f64,
    pub a: f64,
    pub b: f64,
    pub c: f64,
    pub d: f64,
}

impl ParametricCurve {
    /// The curve of sRGB, which Display P3 has too.
    pub const SRGB: ParametricCurve = ParametricCurve {
        g: 2.4,
        a: 1.0 / 1.055,
        b: 0.055 / 1.055,
        c: 1.0 / 12.92,
        d: 0.04045,
    };

    /// The curve of Rec. ITU-R BT.2020 (the one of the 12 bit system, as in CSS Color 4).
    pub const REC2020: ParametricCurve = ParametricCurve {
        g: 1.0 / 0.45,
        a: 1.0 / 1.099_296_826_809_44,
        b: 0.099_296_826_809_44 / 1.099_296_826_809_44,
        c: 1.0 / 4.5,
        d: 4.5 * 0.018_053_968_510_807,
    };

    fn tag(self) -> Vec<u8> {
        let mut tag = b"para\0\0\0\0".to_vec();
        tag.extend(3u16.to_be_bytes()); // function type 3
        tag.extend([0u8; 2]);
        for value in [self.g, self.a, self.b, self.c, self.d] {
            tag.extend(s15_fixed16(value));
        }
        tag
    }
}

/// A text of `multiLocalizedUnicodeType` (ICC version 4), in English.
fn multilocalized_tag(text: &str) -> Vec<u8> {
    let units: Vec<u8> = text.encode_utf16().flat_map(u16::to_be_bytes).collect();
    let mut tag = b"mluc\0\0\0\0".to_vec();
    tag.extend(1u32.to_be_bytes()); // one record
    tag.extend(12u32.to_be_bytes()); // of 12 bytes
    tag.extend(*b"enUS");
    tag.extend((units.len() as u32).to_be_bytes());
    tag.extend(28u32.to_be_bytes()); // the text follows the record
    tag.extend(units);
    tag
}

/// The Bradford adaptation from D65 to D50 that the profiles of version 4 state (`chad`).
const CHROMATIC_ADAPTATION: [f64; 9] = [
    1.047_792_8, 0.022_946_9, -0.050_192_3,
    0.029_627_8, 0.990_434_4, -0.017_073_8,
    -0.009_243_0, 0.015_055_2, 0.751_874_3,
];

/// An RGB matrix/TRC profile of ICC version 4.0, as small as the ones of Apple: the colorants
/// adapted to D50 (XYZ of red, green and blue), one parametric curve for the three channels.
pub fn rgb_matrix_profile(
    description: &str,
    red: (f64, f64, f64),
    green: (f64, f64, f64),
    blue: (f64, f64, f64),
    curve: ParametricCurve,
) -> Vec<u8> {
    let mut chad = b"sf32\0\0\0\0".to_vec();
    for value in CHROMATIC_ADAPTATION {
        chad.extend(s15_fixed16(value));
    }
    assemble(
        0x0400_0000,
        vec![
            Tag::new(b"desc", multilocalized_tag(description)),
            Tag::new(b"cprt", multilocalized_tag("Public Domain")),
            Tag::new(b"wtpt", xyz_tag(D50.0, D50.1, D50.2)),
            Tag::new(b"rXYZ", xyz_tag(red.0, red.1, red.2)),
            Tag::new(b"gXYZ", xyz_tag(green.0, green.1, green.2)),
            Tag::new(b"bXYZ", xyz_tag(blue.0, blue.1, blue.2)),
            Tag::shared(b"rTRC", &[b"gTRC", b"bTRC"], curve.tag()),
            Tag::new(b"chad", chad),
        ],
    )
}

/// A profile of Display P3 (the primaries of DCI-P3 with the white and the curve of sRGB).
pub fn display_p3_profile() -> Vec<u8> {
    rgb_matrix_profile(
        "Display P3",
        (0.515_146, 0.241_200, -0.001_050),
        (0.292_010, 0.692_223, 0.041_878),
        (0.157_139, 0.066_577, 0.784_276),
        ParametricCurve::SRGB,
    )
}

/// A profile of Rec. ITU-R BT.2020 with its transfer function (the OETF, which is also the curve of
/// the CSS space `rec2020`).
///
/// Not right on iOS: measured on an iPhone 16 Pro, the device decodes the coordinates of this profile
/// with a pure gamma of 2.4 and not with this curve (the fit is 0.14 CIEDE2000 against 5.4). A profile
/// with the primaries of Rec. 2020 and the gamma 2.4 itself is right there (0.00):
/// [`rec2020_gamma24_profile`].
pub fn rec2020_profile() -> Vec<u8> {
    rgb_matrix_profile(
        "Rec. ITU-R BT.2020",
        (0.673_515, 0.279_059, -0.001_932),
        (0.165_697, 0.675_318, 0.029_978),
        (0.125_083, 0.045_623, 0.797_059),
        ParametricCurve::REC2020,
    )
}

/// A profile with the primaries of Rec. ITU-R BT.2020 and a pure gamma of 2.4 (BT.1886), which an iPhone
/// shows as the profile says. The coordinates of a colour are the linear ones to the power of 1/2.4.
pub fn rec2020_gamma24_profile() -> Vec<u8> {
    rgb_matrix_profile(
        "Rec. 2020 gamma 2.4",
        (0.673_515, 0.279_059, -0.001_932),
        (0.165_697, 0.675_318, 0.029_978),
        (0.125_083, 0.045_623, 0.797_059),
        ParametricCurve { g: 2.4, a: 1.0, b: 0.0, c: 0.0, d: 0.0 },
    )
}

/// What the header of an ICC profile says (ICC.1, 7.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProfileInfo {
    /// The number of colour components of the data colour space: 1 (`GRAY`), 3 (`RGB `) or 4 (`CMYK`).
    pub components: u32,
    /// The major version of the profile (2 or 4).
    pub major_version: u8,
    /// The class of the profile: `mntr`, `scnr`, `prtr` or `spac`, the ones that PDF takes.
    pub class: [u8; 4],
}

/// Reads the header of an ICC profile and says if a PDF can use it as the colour space of an image
/// or of colours (`ICCBased`, ISO 32000-1, 8.6.5.5): the size and the `acsp` signature must be right,
/// the class one of input, display, output or colour space, the data colour space gray, RGB or
/// CMYK, the profile connection space XYZ or Lab, and the major version 2 to 4. It does not check the tags.
pub fn inspect_profile(profile: &[u8]) -> Result<ProfileInfo, &'static str> {
    if profile.len() < 128 {
        return Err("shorter than the header of 128 bytes");
    }
    let size = u32::from_be_bytes([profile[0], profile[1], profile[2], profile[3]]) as usize;
    if size != profile.len() {
        return Err("the size in the header is not the size of the profile");
    }
    if &profile[36..40] != b"acsp" {
        return Err("no `acsp` signature");
    }
    let major_version = profile[8];
    if !(2..=4).contains(&major_version) {
        return Err("version is not 2, 3 or 4");
    }
    let class = [profile[12], profile[13], profile[14], profile[15]];
    if ![*b"mntr", *b"scnr", *b"prtr", *b"spac"].contains(&class) {
        return Err("not an input, display, output or colour space profile");
    }
    let components = match &profile[16..20] {
        b"GRAY" => 1,
        b"RGB " => 3,
        b"CMYK" => 4,
        _ => return Err("the data colour space is not gray, RGB or CMYK"),
    };
    if &profile[20..24] != b"XYZ " && &profile[20..24] != b"Lab " {
        return Err("the connection space is not XYZ or Lab");
    }
    Ok(ProfileInfo { components, major_version, class })
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
        // the bytes of the profile that the PDF/A files have an output intent of
        let profile = srgb_profile();
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        for byte in &profile {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x100_0000_01b3);
        }
        assert_eq!((profile.len(), hash), (2512, 0xce69_b741_9ca4_43d1));
    }

    #[test]
    fn the_profiles_of_version_4_are_well_formed() {
        for (profile, name) in [(display_p3_profile(), "Display P3"), (rec2020_profile(), "Rec. ITU-R BT.2020")] {
            let u32_at = |at: usize| u32::from_be_bytes(profile[at..at + 4].try_into().unwrap()) as usize;
            assert_eq!(u32_at(0), profile.len());
            assert_eq!(profile.len() % 4, 0);
            assert_eq!(&profile[8..12], &[4, 0, 0, 0]);
            assert_eq!(&profile[12..24], b"mntrRGB XYZ ");
            assert_eq!(&profile[36..40], b"acsp");
            // the tags are inside the profile, at a multiple of 4 bytes
            let tags = u32_at(128);
            assert_eq!(tags, 10);
            let mut signatures = vec![];
            for tag in 0..tags {
                let (offset, size) = (u32_at(132 + 12 * tag + 4), u32_at(132 + 12 * tag + 8));
                assert_eq!(offset % 4, 0);
                assert!(offset + size <= profile.len());
                signatures.push(profile[132 + 12 * tag..136 + 12 * tag].to_vec());
            }
            for wanted in [&b"desc"[..], b"cprt", b"wtpt", b"rXYZ", b"gXYZ", b"bXYZ", b"rTRC", b"gTRC", b"bTRC", b"chad"] {
                assert!(signatures.iter().any(|signature| signature == wanted), "{name} has no {:?}", wanted);
            }
            // as small as the profiles of Apple
            assert!(profile.len() < 700, "{name} has {} bytes", profile.len());
        }
        // the three curves are one
        let profile = display_p3_profile();
        let offsets: Vec<usize> = (0..10)
            .filter(|tag| profile[132 + 12 * tag..136 + 12 * tag].ends_with(b"TRC"))
            .map(|tag| u32::from_be_bytes(profile[136 + 12 * tag..140 + 12 * tag].try_into().unwrap()) as usize)
            .collect();
        assert_eq!(offsets.len(), 3);
        assert!(offsets.iter().all(|offset| *offset == offsets[0]));
    }

    #[test]
    fn the_curves_agree_with_their_transfer_functions() {
        let eotf = |curve: ParametricCurve, x: f64| if x >= curve.d { (curve.a * x + curve.b).powf(curve.g) } else { curve.c * x };
        let srgb = |x: f64| if x <= 0.04045 { x / 12.92 } else { ((x + 0.055) / 1.055).powf(2.4) };
        for x in [0.0, 0.02, 0.04045, 0.1, 0.5, 0.9, 1.0] {
            assert!((eotf(ParametricCurve::SRGB, x) - srgb(x)).abs() < 1e-6, "{}", x);
        }
        // continuous at the knee, and 1 maps to 1
        for curve in [ParametricCurve::SRGB, ParametricCurve::REC2020] {
            assert!((eotf(curve, curve.d - 1e-12) - eotf(curve, curve.d)).abs() < 1e-6);
            assert!((eotf(curve, 1.0) - 1.0).abs() < 1e-6);
        }
    }
}

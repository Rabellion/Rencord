#![allow(clippy::excessive_precision)]
//! sRGB colors, WCAG contrast, and OKLCH conversions.
//!
//! OKLab follows Björn Ottosson's public-domain matrices. Contrast follows
//! the WCAG 2 relative-luminance definition. Nothing here is copied from
//! another Discord client.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Oklch {
    pub l: f32,
    pub c: f32,
    pub h: f32,
}

impl Color {
    pub const fn rgb(hex: u32) -> Self {
        Self {
            r: ((hex >> 16) & 0xff) as u8,
            g: ((hex >> 8) & 0xff) as u8,
            b: (hex & 0xff) as u8,
            a: 255,
        }
    }

    pub const fn rgba(hex: u32) -> Self {
        Self {
            r: ((hex >> 24) & 0xff) as u8,
            g: ((hex >> 16) & 0xff) as u8,
            b: ((hex >> 8) & 0xff) as u8,
            a: (hex & 0xff) as u8,
        }
    }

    pub const fn from_bytes(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    pub fn parse(text: &str) -> Option<Self> {
        let value = text.trim();
        let value = value.strip_prefix('#').unwrap_or(value);
        let value = value
            .strip_prefix("0x")
            .or_else(|| value.strip_prefix("0X"))
            .unwrap_or(value);
        if !value.chars().all(|ch| ch.is_ascii_hexdigit()) {
            return None;
        }
        match value.len() {
            3 | 4 => {
                let digits: Vec<u8> = value.bytes().collect();
                let expand = |index: usize| {
                    let nibble = hex_nibble(digits[index])?;
                    Some(nibble * 17)
                };
                Some(Self {
                    r: expand(0)?,
                    g: expand(1)?,
                    b: expand(2)?,
                    a: if digits.len() == 4 { expand(3)? } else { 255 },
                })
            }
            6 | 8 => {
                let packed = u32::from_str_radix(value, 16).ok()?;
                if value.len() == 6 {
                    Some(Self::rgb(packed))
                } else {
                    Some(Self::rgba(packed))
                }
            }
            _ => None,
        }
    }

    pub fn to_u32(self) -> u32 {
        ((self.r as u32) << 16) | ((self.g as u32) << 8) | self.b as u32
    }

    pub fn to_rgba_u32(self) -> u32 {
        ((self.r as u32) << 24) | ((self.g as u32) << 16) | ((self.b as u32) << 8) | self.a as u32
    }

    pub fn to_hex(self) -> String {
        if self.a == 255 {
            format!("#{:02X}{:02X}{:02X}", self.r, self.g, self.b)
        } else {
            format!("#{:02X}{:02X}{:02X}{:02X}", self.r, self.g, self.b, self.a)
        }
    }

    pub fn with_alpha(self, alpha: u8) -> Self {
        Self { a: alpha, ..self }
    }

    pub fn relative_luminance(self) -> f32 {
        let r = linearize(self.r);
        let g = linearize(self.g);
        let b = linearize(self.b);
        0.2126 * r + 0.7152 * g + 0.0722 * b
    }

    pub fn is_light(self) -> bool {
        self.relative_luminance() > 0.45
    }

    pub fn mix(self, other: Self, t: f32) -> Self {
        self.lerp(other, t)
    }

    pub fn lerp(self, other: Self, t: f32) -> Self {
        let t = t.clamp(0.0, 1.0);
        let (l1, a1, b1) = self.to_oklab();
        let (l2, a2, b2) = other.to_oklab();
        let color = Self::from_oklab(l1 + (l2 - l1) * t, a1 + (a2 - a1) * t, b1 + (b2 - b1) * t);
        let alpha = (self.a as f32 + (other.a as f32 - self.a as f32) * t).round() as u8;
        Self { a: alpha, ..color }
    }

    pub fn to_oklch(self) -> Oklch {
        let (l, a, b) = self.to_oklab();
        let c = (a * a + b * b).sqrt();
        let mut h = b.atan2(a).to_degrees();
        if h < 0.0 {
            h += 360.0;
        }
        if c < 1e-6 {
            h = 0.0;
        }
        Oklch { l, c, h }
    }

    pub fn from_oklch(ok: Oklch) -> Self {
        let mut lo = 0.0;
        let mut hi = ok.c.max(0.0);
        let mut best = oklch_to_srgb(Oklch { c: 0.0, ..ok });
        if channels_in_gamut(oklch_linear(ok)) {
            return oklch_to_srgb(ok);
        }
        for _ in 0..18 {
            let mid = (lo + hi) * 0.5;
            let candidate = Oklch { c: mid, ..ok };
            if channels_in_gamut(oklch_linear(candidate)) {
                best = oklch_to_srgb(candidate);
                lo = mid;
            } else {
                hi = mid;
            }
        }
        best
    }

    pub fn with_chroma_scale(self, scale: f32) -> Self {
        let mut ok = self.to_oklch();
        ok.c *= scale.clamp(0.0, 1.5);
        let color = Self::from_oklch(ok);
        Self { a: self.a, ..color }
    }

    pub fn shift_lightness(self, delta: f32) -> Self {
        let mut ok = self.to_oklch();
        ok.l = (ok.l + delta).clamp(0.0, 1.0);
        let color = Self::from_oklch(ok);
        Self { a: self.a, ..color }
    }

    fn to_oklab(self) -> (f32, f32, f32) {
        let r = linearize(self.r);
        let g = linearize(self.g);
        let b = linearize(self.b);
        let l = 0.412_221_470_8 * r + 0.536_332_536_3 * g + 0.051_445_992_9 * b;
        let m = 0.211_903_498_2 * r + 0.680_699_545_1 * g + 0.107_396_956_6 * b;
        let s = 0.088_302_461_9 * r + 0.281_718_837_6 * g + 0.629_978_700_5 * b;
        let l_ = l.cbrt();
        let m_ = m.cbrt();
        let s_ = s.cbrt();
        let lab_l = 0.210_454_255_3 * l_ + 0.793_617_785_0 * m_ - 0.004_072_046_8 * s_;
        let lab_a = 1.977_998_495_1 * l_ - 2.428_592_205_0 * m_ + 0.450_593_709_9 * s_;
        let lab_b = 0.025_904_037_1 * l_ + 0.782_771_766_2 * m_ - 0.808_675_766_0 * s_;
        (lab_l, lab_a, lab_b)
    }

    fn from_oklab(l: f32, a: f32, b: f32) -> Self {
        let l_ = l + 0.396_337_777_4 * a + 0.215_803_757_3 * b;
        let m_ = l - 0.105_561_345_8 * a - 0.063_854_172_8 * b;
        let s_ = l - 0.089_484_177_5 * a - 1.291_485_548_0 * b;
        let l = l_ * l_ * l_;
        let m = m_ * m_ * m_;
        let s = s_ * s_ * s_;
        let r = 4.076_741_662_1 * l - 3.307_711_591_3 * m + 0.230_969_929_2 * s;
        let g = -1.268_438_004_6 * l + 2.609_757_401_1 * m - 0.341_319_396_5 * s;
        let b = -0.004_196_086_3 * l - 0.703_418_614_7 * m + 1.707_614_701_0 * s;
        Self {
            r: encode_channel(r),
            g: encode_channel(g),
            b: encode_channel(b),
            a: 255,
        }
    }
}

pub fn contrast_ratio(foreground: Color, background: Color) -> f32 {
    let lighter = foreground
        .relative_luminance()
        .max(background.relative_luminance());
    let darker = foreground
        .relative_luminance()
        .min(background.relative_luminance());
    (lighter + 0.05) / (darker + 0.05)
}

/// Walk lightness in OKLCH until `foreground` reaches `minimum` against `background`.
pub fn ensure_contrast(foreground: Color, background: Color, minimum: f32) -> Color {
    if contrast_ratio(foreground, background) + 0.01 >= minimum {
        return foreground;
    }
    let ok = foreground.to_oklch();
    let prefer_lighter = background.relative_luminance() < 0.5;
    let mut best = if prefer_lighter {
        Color::rgb(0xFF_FF_FF)
    } else {
        Color::rgb(0x00_00_00)
    };
    let mut found = false;
    let (mut lo, mut hi) = if prefer_lighter {
        (ok.l, 1.0)
    } else {
        (0.0, ok.l)
    };
    for _ in 0..20 {
        let mid = (lo + hi) * 0.5;
        let candidate = Color::from_oklch(Oklch {
            l: mid,
            c: ok.c,
            h: ok.h,
        })
        .with_alpha(foreground.a);
        if contrast_ratio(candidate, background) + 0.01 >= minimum {
            best = candidate;
            found = true;
            if prefer_lighter {
                hi = mid;
            } else {
                lo = mid;
            }
        } else if prefer_lighter {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    if !found || contrast_ratio(best, background) + 0.01 < minimum {
        let white = Color::rgb(0xFF_FF_FF).with_alpha(foreground.a);
        let black = Color::rgb(0x00_00_00).with_alpha(foreground.a);
        if contrast_ratio(white, background) >= contrast_ratio(black, background) {
            white
        } else {
            black
        }
    } else {
        best
    }
}

pub fn best_ink(background: Color) -> Color {
    let white = Color::rgb(0xFF_FF_FF);
    let black = Color::rgb(0x00_00_00);
    if contrast_ratio(white, background) >= contrast_ratio(black, background) {
        white
    } else {
        black
    }
}

fn linearize(channel: u8) -> f32 {
    let value = channel as f32 / 255.0;
    if value <= 0.04045 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}

fn encode_channel(linear: f32) -> u8 {
    let linear = linear.clamp(0.0, 1.0);
    let encoded = if linear <= 0.003_130_8 {
        linear * 12.92
    } else {
        linear.powf(1.0 / 2.4) * 1.055 - 0.055
    };
    (encoded * 255.0).round().clamp(0.0, 255.0) as u8
}

fn oklch_linear(ok: Oklch) -> (f32, f32, f32) {
    let h = ok.h.to_radians();
    let a = ok.c * h.cos();
    let b = ok.c * h.sin();
    let l_ = ok.l + 0.396_337_777_4 * a + 0.215_803_757_3 * b;
    let m_ = ok.l - 0.105_561_345_8 * a - 0.063_854_172_8 * b;
    let s_ = ok.l - 0.089_484_177_5 * a - 1.291_485_548_0 * b;
    let l = l_ * l_ * l_;
    let m = m_ * m_ * m_;
    let s = s_ * s_ * s_;
    let r = 4.076_741_662_1 * l - 3.307_711_591_3 * m + 0.230_969_929_2 * s;
    let g = -1.268_438_004_6 * l + 2.609_757_401_1 * m - 0.341_319_396_5 * s;
    let b = -0.004_196_086_3 * l - 0.703_418_614_7 * m + 1.707_614_701_0 * s;
    (r, g, b)
}

fn channels_in_gamut((r, g, b): (f32, f32, f32)) -> bool {
    (-0.001..=1.001).contains(&r) && (-0.001..=1.001).contains(&g) && (-0.001..=1.001).contains(&b)
}

fn oklch_to_srgb(ok: Oklch) -> Color {
    let (r, g, b) = oklch_linear(ok);
    Color {
        r: encode_channel(r),
        g: encode_channel(g),
        b: encode_channel(b),
        a: 255,
    }
}

fn hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

/// Discord's six default avatar colors, indexed by `(id >> 22) % 6`.
pub fn default_avatar_fill(index: u8) -> u32 {
    const FILLS: [u32; 6] = [0x5865f2, 0x57f287, 0xfee75c, 0xed4245, 0xeb459e, 0x3ba55d];
    FILLS[usize::from(index) % FILLS.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn black_on_white_is_twenty_one() {
        let ratio = contrast_ratio(Color::rgb(0x00_00_00), Color::rgb(0xFF_FF_FF));
        assert!((ratio - 21.0).abs() < 0.05, "{ratio}");
    }

    #[test]
    fn oklab_roundtrip_stays_within_a_step() {
        for hex in [
            0x00_00_00, 0xFF_FF_FF, 0x4F_6B_FF, 0xA8_B8_FF, 0x58_65_F2, 0xE6_E9_F0,
        ] {
            let color = Color::rgb(hex);
            let back = Color::from_oklch(color.to_oklch());
            assert!(
                (color.r as i16 - back.r as i16).abs() <= 1,
                "{color:?} {back:?}"
            );
            assert!(
                (color.g as i16 - back.g as i16).abs() <= 1,
                "{color:?} {back:?}"
            );
            assert!(
                (color.b as i16 - back.b as i16).abs() <= 1,
                "{color:?} {back:?}"
            );
        }
    }

    #[test]
    fn ensure_contrast_lifts_gray_on_white() {
        let lifted = ensure_contrast(Color::rgb(0xB0_B0_B0), Color::rgb(0xFF_FF_FF), 4.5);
        assert!(contrast_ratio(lifted, Color::rgb(0xFF_FF_FF)) >= 4.5);
    }

    #[test]
    fn hex_parse_and_format() {
        assert_eq!(Color::parse("#4F6BFF").unwrap().to_u32(), 0x4F_6B_FF);
        assert_eq!(Color::parse("4f6bff").unwrap().to_hex(), "#4F6BFF");
        assert_eq!(Color::rgba(0x0000_0099).a, 0x99);
    }
}

//! Conversions between the clipboard's native formats and what the vault stores: `CF_HTML` fragments and
//! DIB bitmaps <-> PNG. Pure functions, no Windows calls.

/// The HTML fragment inside a `CF_HTML` ("HTML Format") buffer.
pub fn html_fragment(data: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(data);
    let field = |name: &str| -> Option<usize> {
        let at = text.find(name)? + name.len();
        let digits: String = text[at..].chars().take_while(char::is_ascii_digit).collect();
        digits.parse().ok()
    };
    if let (Some(s), Some(e)) = (field("StartFragment:"), field("EndFragment:"))
        && s < e
        && e <= data.len()
    {
        return Some(String::from_utf8_lossy(&data[s..e]).into_owned());
    }
    let s = text.find("<!--StartFragment-->")? + "<!--StartFragment-->".len();
    let e = text.find("<!--EndFragment-->")?;
    (s <= e).then(|| text[s..e].to_owned())
}

/// A `CF_HTML` buffer (with its byte-offset header) around `fragment`.
pub fn build_cf_html(fragment: &str) -> Vec<u8> {
    const HEAD: &str = "<html><body>\r\n<!--StartFragment-->";
    const TAIL: &str = "<!--EndFragment-->\r\n</body></html>";
    let header = |sh: usize, eh: usize, sf: usize, ef: usize| {
        format!(
            "Version:0.9\r\nStartHTML:{sh:010}\r\nEndHTML:{eh:010}\r\nStartFragment:{sf:010}\r\nEndFragment:{ef:010}\r\n"
        )
    };
    let hlen = header(0, 0, 0, 0).len();
    let sh = hlen;
    let sf = sh + HEAD.len();
    let ef = sf + fragment.len();
    let eh = ef + TAIL.len();
    let mut out = header(sh, eh, sf, ef).into_bytes();
    out.extend_from_slice(HEAD.as_bytes());
    out.extend_from_slice(fragment.as_bytes());
    out.extend_from_slice(TAIL.as_bytes());
    out
}

pub struct Rgba {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

/// Decodes a `CF_DIB` / `CF_DIBV5` buffer (24- or 32-bit, uncompressed or bitfields).
pub fn dib_to_rgba(dib: &[u8]) -> Option<Rgba> {
    let u32_at = |o: usize| dib.get(o..o + 4).map(|b| u32::from_le_bytes(b.try_into().unwrap()));
    let header = u32_at(0)? as usize;
    if header < 40 || dib.len() < header {
        return None;
    }
    let width = u32_at(4)? as i32;
    let height = u32_at(8)? as i32;
    let bpp = u16::from_le_bytes(dib.get(14..16)?.try_into().ok()?);
    let compression = u32_at(16)?;
    if width <= 0 || height == 0 || !matches!(bpp, 24 | 32) || !matches!(compression, 0 | 3) {
        return None;
    }
    let (w, h, top_down) = (width as usize, height.unsigned_abs() as usize, height < 0);
    if w.checked_mul(h)? > 1 << 28 {
        return None;
    }
    let mut data_off = header;
    let mut alpha_mask = 0u32;
    if compression == 3 {
        // masks live in the V4/V5 header, or right after a 40-byte one
        let masks = if header >= 56 { 40 } else { header };
        alpha_mask = if header >= 56 { u32_at(masks + 12)? } else { 0 };
        if header == 40 {
            data_off += 12;
        }
    }
    let stride = (w * usize::from(bpp)).div_ceil(32) * 4;
    let body = dib.get(data_off..)?;
    if body.len() < stride * (h - 1) + w * usize::from(bpp) / 8 {
        return None;
    }
    let mut px = vec![0u8; w * h * 4];
    let mut any_alpha = false;
    for row in 0..h {
        let src_row = if top_down { row } else { h - 1 - row };
        let src = &body[src_row * stride..];
        for x in 0..w {
            let o = (row * w + x) * 4;
            match bpp {
                24 => px[o..o + 4].copy_from_slice(&[src[x * 3 + 2], src[x * 3 + 1], src[x * 3], 255]),
                _ => {
                    let a = src[x * 4 + 3];
                    any_alpha |= a != 0;
                    px[o..o + 4].copy_from_slice(&[src[x * 4 + 2], src[x * 4 + 1], src[x * 4], a]);
                }
            }
        }
    }
    // 32-bit bitmaps without a real alpha channel carry zeros there: treat them as opaque
    if bpp == 32 && (alpha_mask == 0 || !any_alpha) {
        px.as_chunks_mut::<4>().0.iter_mut().for_each(|p| p[3] = 255);
    }
    Some(Rgba {
        width: w as u32,
        height: h as u32,
        pixels: px,
    })
}

/// A bottom-up 32-bit `BITMAPINFOHEADER` DIB, for putting an image back on the clipboard.
pub fn rgba_to_dib(img: &Rgba) -> Vec<u8> {
    let (w, h) = (img.width as usize, img.height as usize);
    let mut out = Vec::with_capacity(40 + w * h * 4);
    out.extend_from_slice(&40u32.to_le_bytes());
    out.extend_from_slice(&(w as i32).to_le_bytes());
    out.extend_from_slice(&(h as i32).to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&32u16.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&((w * h * 4) as u32).to_le_bytes());
    out.extend_from_slice(&[0; 16]);
    for row in (0..h).rev() {
        for x in 0..w {
            let p = &img.pixels[(row * w + x) * 4..][..4];
            out.extend_from_slice(&[p[2], p[1], p[0], p[3]]);
        }
    }
    out
}

pub fn png_encode(img: &Rgba) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let mut enc = png::Encoder::new(&mut out, img.width, img.height);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header().ok()?.write_image_data(&img.pixels).ok()?;
    Some(out)
}

pub fn png_decode(data: &[u8]) -> Option<Rgba> {
    let mut dec = png::Decoder::new(std::io::Cursor::new(data));
    dec.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = dec.read_info().ok()?;
    let mut buf = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut buf).ok()?;
    let src = &buf[..info.buffer_size()];
    let pixels = match info.color_type {
        png::ColorType::Rgba => src.to_vec(),
        png::ColorType::Rgb => src
            .as_chunks::<3>()
            .0
            .iter()
            .flat_map(|p| [p[0], p[1], p[2], 255])
            .collect(),
        png::ColorType::GrayscaleAlpha => src
            .as_chunks::<2>()
            .0
            .iter()
            .flat_map(|p| [p[0], p[0], p[0], p[1]])
            .collect(),
        png::ColorType::Grayscale => src.iter().flat_map(|&g| [g, g, g, 255]).collect(),
        png::ColorType::Indexed => return None,
    };
    Some(Rgba {
        width: info.width,
        height: info.height,
        pixels,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Rgba {
        Rgba {
            width: 3,
            height: 2,
            pixels: (0..24).map(|i| (i * 9) as u8).collect(),
        }
    }

    #[test]
    fn html_roundtrip() {
        let frag = "<b>привет</b> <i>мир</i>";
        let buf = build_cf_html(frag);
        assert_eq!(html_fragment(&buf).as_deref(), Some(frag));
        assert!(String::from_utf8_lossy(&buf).starts_with("Version:0.9"));
    }

    #[test]
    fn html_without_offsets_uses_markers() {
        let buf = b"<html><!--StartFragment-->x<!--EndFragment--></html>";
        assert_eq!(html_fragment(buf).as_deref(), Some("x"));
        assert_eq!(html_fragment(b"nothing"), None);
    }

    #[test]
    fn dib_png_roundtrip() {
        let mut img = sample();
        img.pixels.as_chunks_mut::<4>().0.iter_mut().for_each(|p| p[3] = 200);
        let dib = rgba_to_dib(&img);
        let back = dib_to_rgba(&dib).unwrap();
        assert_eq!((back.width, back.height), (3, 2));
        // 32-bit BI_RGB is read as opaque, colours must match
        for (a, b) in img
            .pixels
            .as_chunks::<4>()
            .0
            .iter()
            .zip(back.pixels.as_chunks::<4>().0.iter())
        {
            assert_eq!(a[..3], b[..3]);
        }
        let png = png_encode(&img).unwrap();
        let dec = png_decode(&png).unwrap();
        assert_eq!(dec.pixels, img.pixels);
    }

    #[test]
    fn rejects_garbage() {
        assert!(dib_to_rgba(&[0; 10]).is_none());
        assert!(dib_to_rgba(&[0; 64]).is_none());
        assert!(png_decode(b"not a png").is_none());
    }
}

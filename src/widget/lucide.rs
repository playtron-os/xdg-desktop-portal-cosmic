//! Vendored Lucide glyphs. The portal draws these instead of icon-theme names
//! so its chrome looks the same whatever icon theme is installed. The SVGs
//! under `res/icons/lucide` are copied byte-identical from icetron-themes and
//! are never edited here; the bytes are vendored rather than depended on so a
//! second `iced_core` is not pulled into this libcosmic binary.

/// `symbolic` is what routes the glyph through `cosmic::theme::Svg` recoloring
/// (the vendored SVGs stroke with `currentColor`); without it a selected tool
/// silently stops taking the accent tint.
pub fn icon(bytes: &'static [u8], size: u16) -> cosmic::widget::icon::Icon {
    let mut handle = cosmic::widget::icon::from_svg_bytes(bytes);
    handle.symbolic = true;
    cosmic::widget::icon::icon(handle).size(size)
}

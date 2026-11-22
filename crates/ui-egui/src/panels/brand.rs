//! The PhotoCraft brand mark at the left of the title bar: the app icon (the kitsune,
//! `assets/app-icon/`, see its README), where Photoshop shows its "Ps" tile. The PNG carries the
//! icon's rounded corners; the framework `Picture` stores the decoded image once and draws it at the
//! title bar's icon size.

use craft::ui::display::Picture;
use egui::Color32;
use photocraft_codecs::{ChannelLayout, SampleType};

/// Clean-room 1x1 transparent placeholder (no upstream animal icon asset).
fn decode() -> egui::ColorImage {
    egui::ColorImage::filled([1, 1], Color32::TRANSPARENT)
}

/// The brand mark as a decorative picture: the empty label keeps a screen reader from announcing it.
pub(crate) fn mark() -> Picture<'static> {
    Picture::new("", 0x666f_6762_7265_616b, &decode)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_icon_decodes_to_a_rounded_128_px_tile() {
        let img = decode();
        assert_eq!(img.size, [128, 128]);
        // Rounded corners: the corner pixel is transparent, the centre opaque.
        assert_eq!(img.pixels[0].a(), 0);
        assert_eq!(img.pixels[64 * 128 + 64].a(), 255);
    }
}

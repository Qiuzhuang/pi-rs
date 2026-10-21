//! Image component tests (ported from terminal-image.test.ts). Single test: process-global state.

use std::sync::{Arc, Mutex};

use base64::Engine;
use pi_tui::components::image::{Image, ImageOptions, ImageTheme, set_image_transcoder};
use pi_tui::terminal_image::*;
use pi_tui::tui::Component;
use pi_tui::utils::visible_width;

fn plain_theme() -> ImageTheme {
    ImageTheme { fallback_color: Arc::new(|v: &str| v.to_string()) }
}

fn kitty(cell: (u32, u32)) {
    set_capabilities(TerminalCapabilities { images: Some(ImageProtocol::Kitty), true_color: true, hyperlinks: true });
    set_cell_dimensions(CellDimensions { width_px: cell.0, height_px: cell.1 });
}

#[test]
fn image_component() {
    // Height capped to a square pixel box by default.
    kitty((10, 20));
    let image = Image::new("AAAA", "image/png", plain_theme(), ImageOptions { max_width_cells: Some(10), ..Default::default() }, Some(ImageDimensions { width_px: 10, height_px: 100 }));
    let lines = image.render(12);
    assert_eq!(lines.len(), 5);
    assert!(lines[0].contains(",c=1,r=5"));

    // Sequence on the first line with empty padding rows.
    kitty((10, 10));
    let image = Image::new("AAAA", "image/png", plain_theme(), ImageOptions { max_width_cells: Some(2), ..Default::default() }, Some(ImageDimensions { width_px: 20, height_px: 20 }));
    let lines = image.render(4);
    let id = image.get_image_id().unwrap();
    assert!(lines[0].starts_with("\x1b_G"));
    assert!(lines[0].contains(",C=1,"));
    assert!(lines[0].contains(&format!(",i={id}")));
    assert!(lines[0].ends_with("\x1b\\"));
    assert_eq!(&lines[1..], [""]);

    // Long fallback lines are truncated.
    set_capabilities(TerminalCapabilities { images: None, true_color: false, hyperlinks: false });
    let home = std::env::var("HOME").unwrap();
    let long = format!("{home}/images/{}.png", "generated-image-with-a-very-long-absolute-path".repeat(4));
    let image = Image::new(
        "AAAA",
        "image/png",
        ImageTheme { fallback_color: Arc::new(|v: &str| format!("\x1b[33m{v}\x1b[0m")) },
        ImageOptions { filename: Some(long), ..Default::default() },
        Some(ImageDimensions { width_px: 1280, height_px: 720 }),
    );
    let lines = image.render(40);
    assert_eq!(lines.len(), 1);
    assert!(visible_width(&lines[0]) <= 40);
    assert!(lines[0].contains("...") && lines[0].contains('~'));

    // Transcoding.
    let b64 = |b: &[u8]| base64::engine::general_purpose::STANDARD.encode(b);
    let jpeg = b64(b"jpeg");
    let png = b64(&[0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 0x0d, 0x49, 0x48, 0x44, 0x52, 0, 0, 0, 0x28, 0, 0, 0, 0x0a]);
    let calls = Arc::new(Mutex::new(Vec::<String>::new()));
    let transcoder = {
        let calls = calls.clone();
        let (jpeg, png) = (jpeg.clone(), png.clone());
        Arc::new(move |data: &str, _mime: &str| {
            calls.lock().unwrap().push(data.to_string());
            if data == jpeg { Some(png.clone()) } else { None }
        })
    };
    let render = |data: &str, mime: &str| {
        Image::new(data, mime, plain_theme(), ImageOptions::default(), Some(ImageDimensions { width_px: 20, height_px: 20 })).render(20)
    };
    kitty((10, 10));
    set_image_transcoder(Some(transcoder.clone()));
    let lines = render(&jpeg, "image/jpeg");
    assert!(lines[0].contains("f=100") && lines[0].contains(&format!(";{png}\x1b\\")));
    assert_eq!(lines.len(), 5);

    set_image_transcoder(None);
    let image = Image::new(jpeg.clone(), "image/jpeg", plain_theme(), ImageOptions::default(), None);
    assert!(image.render(80)[0].starts_with("[Image: [image/jpeg]"));
    set_image_transcoder(Some(Arc::new(|_: &str, _: &str| None)));
    image.invalidate();
    assert!(image.render(80)[0].starts_with("[Image: [image/jpeg]"));
    set_image_transcoder(Some(transcoder.clone()));
    image.invalidate();
    assert!(image.render(80)[0].contains("\x1b_G"));

    calls.lock().unwrap().clear();
    set_image_transcoder(Some(transcoder.clone()));
    let image = Image::new(jpeg.clone(), "image/jpeg", plain_theme(), ImageOptions::default(), None);
    image.render(80);
    render(&jpeg, "image/jpeg");
    for i in 0..40 {
        render(&format!("other-{i}"), "image/jpeg");
    }
    image.invalidate();
    image.render(40);
    assert_eq!(calls.lock().unwrap().iter().filter(|c| **c == jpeg).count(), 1);

    calls.lock().unwrap().clear();
    assert!(render(&png, "image/png")[0].contains(&format!(";{png}\x1b\\")));
    set_capabilities(TerminalCapabilities { images: Some(ImageProtocol::Iterm2), true_color: true, hyperlinks: true });
    assert!(render(&jpeg, "image/jpeg").last().unwrap().ends_with(&format!(":{jpeg}\x07")));
    assert!(calls.lock().unwrap().is_empty());
    set_image_transcoder(None);
}

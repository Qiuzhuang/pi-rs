//! Port of `packages/coding-agent/src/utils/photon.ts`.
//!
//! The TS version lazily loads Photon (Rust compiled to WASM) and patches the WASM file lookup for
//! Bun binaries. The Rust port processes images natively with the `image` crate, so "loading" always
//! succeeds. [`Photon`] and [`PhotonImage`] mirror the slice of the Photon API pi uses
//! (`PhotonImage.new_from_byteslice`, `get_width`, `get_height`, `get_raw_pixels`, `get_bytes`,
//! `get_bytes_jpeg`, `resize`, `fliph`, `flipv`).

use image::{ImageEncoder, RgbaImage};

/// An RGBA image (`PhotonImage`).
#[derive(Debug, Clone, PartialEq)]
pub struct PhotonImage {
    image: RgbaImage,
}

pub type PhotonImageType = PhotonImage;

impl PhotonImage {
    /// `new PhotonImage(rawPixels, width, height)`.
    pub fn new(raw_pixels: Vec<u8>, width: u32, height: u32) -> anyhow::Result<Self> {
        let image = RgbaImage::from_raw(width, height, raw_pixels)
            .ok_or_else(|| anyhow::anyhow!("raw pixel buffer does not match {width}x{height}"))?;
        Ok(Self { image })
    }

    /// `PhotonImage.new_from_byteslice(bytes)`: decode any supported format.
    pub fn new_from_byteslice(bytes: &[u8]) -> anyhow::Result<Self> {
        let decoded = image::load_from_memory(bytes)?;
        Ok(Self {
            image: decoded.to_rgba8(),
        })
    }

    pub fn get_width(&self) -> u32 {
        self.image.width()
    }

    pub fn get_height(&self) -> u32 {
        self.image.height()
    }

    /// RGBA pixels, row-major.
    pub fn get_raw_pixels(&self) -> &[u8] {
        self.image.as_raw()
    }

    /// PNG-encoded bytes.
    pub fn get_bytes(&self) -> anyhow::Result<Vec<u8>> {
        let mut out = Vec::new();
        image::codecs::png::PngEncoder::new(&mut out).write_image(
            self.image.as_raw(),
            self.image.width(),
            self.image.height(),
            image::ExtendedColorType::Rgba8,
        )?;
        Ok(out)
    }

    /// JPEG-encoded bytes at `quality` (alpha is dropped).
    pub fn get_bytes_jpeg(&self, quality: u8) -> anyhow::Result<Vec<u8>> {
        let rgb = image::DynamicImage::ImageRgba8(self.image.clone()).to_rgb8();
        let mut out = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, quality.clamp(1, 100)).write_image(
            rgb.as_raw(),
            rgb.width(),
            rgb.height(),
            image::ExtendedColorType::Rgb8,
        )?;
        Ok(out)
    }
}

/// Resampling filters (`SamplingFilter`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SamplingFilter {
    Nearest,
    Triangle,
    CatmullRom,
    Gaussian,
    Lanczos3,
}

impl SamplingFilter {
    fn filter_type(self) -> image::imageops::FilterType {
        use image::imageops::FilterType;
        match self {
            Self::Nearest => FilterType::Nearest,
            Self::Triangle => FilterType::Triangle,
            Self::CatmullRom => FilterType::CatmullRom,
            Self::Gaussian => FilterType::Gaussian,
            Self::Lanczos3 => FilterType::Lanczos3,
        }
    }
}

/// The Photon module (`typeof import("@silvia-odwyer/photon-node")`).
#[derive(Debug, Clone, Copy, Default)]
pub struct Photon;

impl Photon {
    /// `photon.resize(image, width, height, filter)`.
    pub fn resize(&self, image: &PhotonImage, width: u32, height: u32, filter: SamplingFilter) -> PhotonImage {
        PhotonImage {
            image: image::imageops::resize(&image.image, width.max(1), height.max(1), filter.filter_type()),
        }
    }

    /// `photon.fliph(image)` (in place).
    pub fn fliph(&self, image: &mut PhotonImage) {
        image::imageops::flip_horizontal_in_place(&mut image.image);
    }

    /// `photon.flipv(image)` (in place).
    pub fn flipv(&self, image: &mut PhotonImage) {
        image::imageops::flip_vertical_in_place(&mut image.image);
    }
}

static PHOTON: Photon = Photon;

/// Load the photon module asynchronously.
/// Returns cached module on subsequent calls.
pub async fn load_photon() -> anyhow::Result<Option<&'static Photon>> {
    Ok(Some(&PHOTON))
}

/// Synchronous [`load_photon`].
pub fn photon() -> &'static Photon {
    &PHOTON
}

use std::collections::BTreeMap;
use std::fmt;

use image::{GenericImage, Pixel, Rgb};

use hsl::HSL;

/// Vibrancy
///
/// 6 vibrant colors: primary, dark, light, dark muted and light muted.
#[derive(Debug, Hash, PartialEq, Eq, Default)]
pub struct Vibrancy {
  pub primary: Option<Rgb<u8>>,
  pub dark: Option<Rgb<u8>>,
  pub light: Option<Rgb<u8>>,
  pub muted: Option<Rgb<u8>>,
  pub dark_muted: Option<Rgb<u8>>,
  pub light_muted: Option<Rgb<u8>>,
}

impl Vibrancy {
  /// Create new vibrancy map from an image
  pub fn new<P, G>(image: &G) -> Vibrancy
  where
    P: Sized + Pixel<Subpixel = u8>,
    G: Sized + GenericImage<Pixel = P>,
  {
    generate_varation_colors(&Palette::new(image, 256, 10))
  }

  fn color_already_set(&self, color: &Rgb<u8>) -> bool {
    let color = Some(*color);
    self.primary == color
      || self.dark == color
      || self.light == color
      || self.muted == color
      || self.dark_muted == color
      || self.light_muted == color
  }

  fn find_color_variation(
    &self,
    palette: &[Rgb<u8>],
    pixel_counts: &BTreeMap<usize, usize>,
    luma: &MTM<f64>,
    saturation: &MTM<f64>,
  ) -> Option<Rgb<u8>> {
    let mut max = None;
    let mut max_value = 0_f64;

    let complete_population = pixel_counts.values().fold(0, |acc, c| acc + c);

    for (index, swatch) in palette.iter().enumerate() {
      let HSL { h: _, s, l } = HSL::from_rgb(swatch.channels());

      if s >= saturation.min
        && s <= saturation.max
        && l >= luma.min
        && l <= luma.max
        && !self.color_already_set(swatch)
      {
        let population = *pixel_counts.get(&index).unwrap_or(&0) as f64;
        if population == 0_f64 {
          continue;
        }
        let value = create_comparison_value(
          s,
          saturation.target,
          l,
          luma.target,
          population,
          complete_population as f64,
        );
        if max.is_none() || value > max_value {
          max = Some(swatch.clone());
          max_value = value;
        }
      }
    }

    max
  }

  // fn fill_empty_swatches(self) {
  //     if self.primary.is_none() {
  //         // If we do not have a vibrant color...
  //         if let Some(dark) = self.dark {
  //             // ...but we do have a dark vibrant, generate the value by modifying the luma
  //             let hsl = HSL::from_pixel(&dark).clone()
  //             hsl.l = settings::TARGET_NORMAL_LUMA;
  //         }
  //     }
  // }
}

fn generate_varation_colors(p: &Palette) -> Vibrancy {
  let mut vibrancy = Vibrancy::default();
  vibrancy.primary = vibrancy.find_color_variation(
    &p.palette,
    &p.pixel_counts,
    &MTM {
      min: settings::MIN_NORMAL_LUMA,
      target: settings::TARGET_NORMAL_LUMA,
      max: settings::MAX_NORMAL_LUMA,
    },
    &MTM {
      min: settings::MIN_VIBRANT_SATURATION,
      target: settings::TARGET_VIBRANT_SATURATION,
      max: 1_f64,
    },
  );

  vibrancy.light = vibrancy.find_color_variation(
    &p.palette,
    &p.pixel_counts,
    &MTM {
      min: settings::MIN_LIGHT_LUMA,
      target: settings::TARGET_LIGHT_LUMA,
      max: 1_f64,
    },
    &MTM {
      min: settings::MIN_VIBRANT_SATURATION,
      target: settings::TARGET_VIBRANT_SATURATION,
      max: 1_f64,
    },
  );

  vibrancy.dark = vibrancy.find_color_variation(
    &p.palette,
    &p.pixel_counts,
    &MTM {
      min: 0_f64,
      target: settings::TARGET_DARK_LUMA,
      max: settings::MAX_DARK_LUMA,
    },
    &MTM {
      min: settings::MIN_VIBRANT_SATURATION,
      target: settings::TARGET_VIBRANT_SATURATION,
      max: 1_f64,
    },
  );

  vibrancy.muted = vibrancy.find_color_variation(
    &p.palette,
    &p.pixel_counts,
    &MTM {
      min: settings::MIN_NORMAL_LUMA,
      target: settings::TARGET_NORMAL_LUMA,
      max: settings::MAX_NORMAL_LUMA,
    },
    &MTM {
      min: 0_f64,
      target: settings::TARGET_MUTED_SATURATION,
      max: settings::MAX_MUTED_SATURATION,
    },
  );

  vibrancy.light_muted = vibrancy.find_color_variation(
    &p.palette,
    &p.pixel_counts,
    &MTM {
      min: settings::MIN_LIGHT_LUMA,
      target: settings::TARGET_LIGHT_LUMA,
      max: 1_f64,
    },
    &MTM {
      min: 0_f64,
      target: settings::TARGET_MUTED_SATURATION,
      max: settings::MAX_MUTED_SATURATION,
    },
  );

  vibrancy.dark_muted = vibrancy.find_color_variation(
    &p.palette,
    &p.pixel_counts,
    &MTM {
      min: 0_f64,
      target: settings::TARGET_DARK_LUMA,
      max: settings::MAX_DARK_LUMA,
    },
    &MTM {
      min: 0_f64,
      target: settings::TARGET_MUTED_SATURATION,
      max: settings::MAX_MUTED_SATURATION,
    },
  );

  vibrancy
}

fn invert_diff(val: f64, target_val: f64) -> f64 {
  1_f64 - (val - target_val).abs()
}

fn weighted_mean(vals: &[(f64, f64)]) -> f64 {
  let (sum, sum_weight) = vals
    .iter()
    .fold((0_f64, 0_f64), |(sum, sum_weight), &(val, weight)| {
      (sum + val * weight, sum_weight + weight)
    });

  sum / sum_weight
}

fn create_comparison_value(
  sat: f64,
  target_sat: f64,
  luma: f64,
  target_uma: f64,
  population: f64,
  max_population: f64,
) -> f64 {
  weighted_mean(&[
    (invert_diff(sat, target_sat), settings::WEIGHT_SATURATION),
    (invert_diff(luma, target_uma), settings::WEIGHT_LUMA),
    (population / max_population, settings::WEIGHT_POPULATION),
  ])
}

/// Minimum, Maximum, Target
#[derive(Debug, Hash)]
struct MTM<T> {
  min: T,
  target: T,
  max: T,
}

mod settings {

  pub const TARGET_DARK_LUMA: f64 = 0.26;
  pub const MAX_DARK_LUMA: f64 = 0.45;

  pub const MIN_LIGHT_LUMA: f64 = 0.55;
  pub const TARGET_LIGHT_LUMA: f64 = 0.74;

  pub const MIN_NORMAL_LUMA: f64 = 0.3;
  pub const TARGET_NORMAL_LUMA: f64 = 0.5;
  pub const MAX_NORMAL_LUMA: f64 = 0.7;

  pub const TARGET_MUTED_SATURATION: f64 = 0.3;
  pub const MAX_MUTED_SATURATION: f64 = 0.4;

  pub const TARGET_VIBRANT_SATURATION: f64 = 1.0;
  pub const MIN_VIBRANT_SATURATION: f64 = 0.35;

  pub const WEIGHT_SATURATION: f64 = 3.0;
  pub const WEIGHT_LUMA: f64 = 6.0;
  pub const WEIGHT_POPULATION: f64 = 1.0;
}

use color_quant::NeuQuant;
use image::Rgba;
use itertools::Itertools;

/// Palette of colors.
#[derive(Debug, Hash, PartialEq, Eq, Default)]
pub struct Palette {
  /// Palette of Colors represented in RGB
  pub palette: Vec<Rgb<u8>>,
  /// A map of indices in the palette to a count of pixels in approximately that color in the
  /// original image.
  pub pixel_counts: BTreeMap<usize, usize>,
}

impl Palette {
  /// Create a new palett from an image
  ///
  /// Color count and quality are given straight to [color_quant], values should be between
  /// 8...512 and 1...30 respectively. (By the way: 10 is a good default quality.)
  ///
  /// [color_quant]: https://github.com/PistonDevelopers/color_quant
  pub fn new<P, G>(image: &G, color_count: usize, quality: i32) -> Palette
  where
    P: Sized + Pixel<Subpixel = u8>,
    G: Sized + GenericImage<Pixel = P>,
  {
    let pixels: Vec<Rgba<u8>> = image
      .pixels()
      .map(|(_, _, pixel)| pixel.to_rgba())
      .collect();

    let mut flat_pixels: Vec<u8> = Vec::with_capacity(pixels.len());
    for rgba in &pixels {
      if is_boring_pixel(&rgba) {
        continue;
      }

      for subpixel in rgba.channels() {
        flat_pixels.push(*subpixel);
      }
    }

    let quant = NeuQuant::new(quality, color_count, &flat_pixels);

    let pixel_counts = pixels
      .iter()
      .map(|rgba| quant.index_of(&rgba.channels()))
      .fold(BTreeMap::new(), |mut acc, pixel| {
        *acc.entry(pixel).or_insert(0) += 1;
        acc
      });

    let palette: Vec<Rgb<u8>> = quant
      .color_map_rgba()
      .iter()
      .chunks_lazy(4)
      .into_iter()
      .map(|rgba_iter| {
        let rgba_slice: Vec<u8> = rgba_iter.cloned().collect();
        Rgba::from_slice(&rgba_slice).clone().to_rgb()
      })
      .unique()
      .collect();

    Palette {
      palette: palette,
      pixel_counts: pixel_counts,
    }
  }

  fn frequency_of(&self, color: &Rgb<u8>) -> usize {
    let index = self
      .palette
      .iter()
      .position(|x| x.channels() == color.channels());
    if let Some(index) = index {
      *self.pixel_counts.get(&index).unwrap_or(&0)
    } else {
      0
    }
  }

  /// Change ordering of colors in palette to be of frequency using the pixel count.
  pub fn sort_by_frequency(&self) -> Self {
    let mut colors = self.palette.clone();
    colors.sort_by(|a, b| self.frequency_of(&a).cmp(&self.frequency_of(&b)));

    Palette {
      palette: colors,
      pixel_counts: self.pixel_counts.clone(),
    }
  }
}

fn is_boring_pixel(pixel: &Rgba<u8>) -> bool {
  let (r, g, b, a) = (pixel[0], pixel[1], pixel[2], pixel[3]);

  // If pixel is mostly opaque and not white
  const MIN_ALPHA: u8 = 125;
  const MAX_COLOR: u8 = 250;

  let interesting = (a >= MIN_ALPHA) && !(r > MAX_COLOR && g > MAX_COLOR && b > MAX_COLOR);

  !interesting
}

impl fmt::Display for Palette {
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
    let color_list = self
      .palette
      .iter()
      .map(|rgb| format!("#{:02X}{:02X}{:02X}", rgb[0], rgb[1], rgb[2]))
      .join(", ");

    write!(f, "Color Palette {{ {} }}", color_list)
  }
}

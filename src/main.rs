use btleplug::{
  api::{
    bleuuid::uuid_from_u16, Central, CentralEvent, Manager as _, Peripheral as _, ScanFilter,
    WriteType,
  },
  platform::Manager,
};
use color_thief::get_palette;
use dxgcap::DXGIManager;
use futures::stream::StreamExt;
use glam::*;

use image::{
  imageops::FilterType,
  DynamicImage::{self, ImageRgb8},
  ImageBuffer,
};
use palette::{rgb::Rgb, FromColor, Hsl, IntoColor};
use uuid::Uuid;

mod vibrant;

const LIGHT_MAC: u64 = 0xFFFF3A00028F;
const LIGHT_CONTROL_UUID: Uuid = uuid_from_u16(0xFFF1);

const CAPTURE_DEVICE: usize = 1;

const COLOR_GAMMA: f32 = 1.0;
const COLOR_FADE: f32 = 0.8;
const COLOR_CORRECT_LIGHT: f32 = 0.9;
const COLOR_CORRECT_SATURATION: f32 = 0.9;
/*const COLOR_ALGORITHM: ColorSamplingAlgorithm = ColorSamplingAlgorithm::MostDominant {
  quality: 2,
  sorted: true,
};*/
const COLOR_ALGORITHM: ColorSamplingAlgorithm = ColorSamplingAlgorithm::Vibrancy;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
  println!("Starting up and initializing bluetooth connection to light");
  println!("================================================");
  let manager = Manager::new().await.unwrap();
  let adapters = manager.adapters().await?;
  let central = adapters.into_iter().next().unwrap();
  let mut events = central.events().await.unwrap();
  central.start_scan(ScanFilter::default()).await?;

  println!("Waiting for bluetooth light to be discovered...");
  let mut light = None;
  while let Some(event) = events.next().await {
    if let CentralEvent::DeviceDiscovered(id) = event {
      let peripheral = central.peripheral(&id).await.unwrap();
      if peripheral.address() == LIGHT_MAC.try_into().unwrap() {
        println!("Found light at {:?}", id);
        light = Some(peripheral);
        break;
      }

      println!("Found unknown device at {:?}", id);
    }
  }
  central.stop_scan().await?;

  let light = light.unwrap();
  light.connect().await?;
  light.discover_services().await?;
  let chars = light.characteristics();
  println!("Found characteristics in light: {:#?}", chars);
  let cmd_char = chars.iter().find(|c| c.uuid == LIGHT_CONTROL_UUID).unwrap();

  println!("Start capturing frames and set light");
  let mut dxgi = DXGIManager::new(1000000)?;
  dxgi.set_capture_source_index(CAPTURE_DEVICE);
  //dxgi.acquire_output_duplication().unwrap();

  let mut previous_pixel = Vec3::ZERO;
  loop {
    let (buffer, (width, height)) = dxgi
      .capture_frame()
      .map_err(|e| format!("Capturing error: {:?}", e))?;

    let color = match COLOR_ALGORITHM {
      ColorSamplingAlgorithm::SquaredAverage { sample_rate } => {
        let sample_width = (width as f32 * sample_rate) as usize;
        let step_x = width / sample_width;
        let sample_height = (width as f32 * sample_rate) as usize;
        let step_y = height / sample_height;
        let mut sampled_color = Vec3::ZERO;
        let mut samples = 0;
        for x in 0..sample_width {
          for y in 0..sample_height {
            let i = (x * step_x) + width * (y * step_y);
            let bgra = buffer[i];
            sampled_color += Vec3::new(
              (bgra.r as f32).powf(2.0),
              (bgra.g as f32).powf(2.0),
              (bgra.b as f32).powf(2.0),
            );
            samples += 1;
          }
        }

        let avg_color = sampled_color / samples as f32;
        Vec3::new(avg_color.x.sqrt(), avg_color.y.sqrt(), avg_color.z.sqrt()) / 255.0
      }
      ColorSamplingAlgorithm::MostDominant { quality, sorted } => {
        let pixels = buffer
          .iter()
          .flat_map(|pixel| [pixel.r, pixel.g, pixel.b])
          .collect::<Vec<_>>();
        let mut dominant = get_palette(&pixels, color_thief::ColorFormat::Rgb, quality, 2)?;
        if sorted {
          dominant.sort_unstable_by_key(|color| {
            let color = Vec3::new(color.r as f32, color.g as f32, color.b as f32);
            let min = color.min_element() as u8;
            let max = color.max_element() as u8;
            ((max + min) * (max - min)) / max.max(1)
          });
        }
        let dominant = dominant[0];
        let color = Vec3::new(dominant.r as f32, dominant.g as f32, dominant.b as f32);
        color / 255.0
      }
      ColorSamplingAlgorithm::Vibrancy => {
        let pixels = buffer
          .iter()
          .flat_map(|pixel| [pixel.r, pixel.g, pixel.b])
          .collect::<Vec<_>>();
        let image = DynamicImage::ImageRgb8(
          ImageBuffer::from_raw(width as u32, height as u32, pixels).unwrap(),
        )
        .resize(
          (width as f32 * 0.05) as u32,
          (height as f32 * 0.05) as u32,
          FilterType::Nearest,
        );
        let vibrancy = vibrant::Vibrancy::new(&image);
        let color = vibrancy
          .primary
          .or(vibrancy.light)
          .or(vibrancy.light_muted)
          .or(vibrancy.muted)
          .or(vibrancy.dark_muted)
          .or(vibrancy.dark)
          .unwrap_or_else(|| image::Rgb([0, 0, 0]));
        Vec3::new(color.0[0] as f32, color.0[1] as f32, color.0[2] as f32) / 255.0
      }
    };

    let color = color.powf(1.0 / COLOR_GAMMA);
    let mut hsl: Hsl = Rgb::new(color.x, color.y, color.z).into_color();
    hsl.lightness = mix(hsl.lightness, 0.5, COLOR_CORRECT_LIGHT);
    hsl.saturation = mix(hsl.saturation, 1.0, COLOR_CORRECT_SATURATION);
    let rgb: Rgb = hsl.into_color();
    let color = Vec3::new(rgb.red, rgb.green, rgb.blue);
    let color = previous_pixel * COLOR_FADE + color * (1.0 - COLOR_FADE);
    previous_pixel = color;
    let color = (color * 255.0).min(Vec3::splat(255.0));
    println!("Color grabbed {}", color);
    let color_cmd = vec![0x01, color.x as u8, color.y as u8, color.z as u8, 0x64];
    light
      .write(cmd_char, &color_cmd, WriteType::WithoutResponse)
      .await?;
  }
}

fn mix(x: f32, y: f32, weight: f32) -> f32 {
  (x * x * (1.0 - weight) + y * y * weight).sqrt()
}
enum ColorSamplingAlgorithm {
  SquaredAverage { sample_rate: f32 },
  MostDominant { quality: u8, sorted: bool },
  Vibrancy,
}

use std::io::{Error, ErrorKind};
use std::path::PathBuf;
use std::time::Instant;

use clap::Parser;
use const_format::formatcp;
use image::DynamicImage::ImageRgba16;
use image::ImageReader;
use image::{ColorType, DynamicImage, GenericImageView, ImageBuffer, ImageFormat};
use rayon::iter::ParallelIterator;

const GIT_HASH: &str = env!("GIT_HASH");
const GIT_BRANCH: &str = env!("GIT_BRANCH");
const GIT_VERSION: &str = env!("GIT_VERSION");
const BUILD_DATE: &str = env!("BUILD_DATE");

const CLAP_VERSION: &str = formatcp!("{GIT_VERSION} [{GIT_BRANCH}, {GIT_HASH}, {BUILD_DATE}]");

#[derive(clap::ValueEnum, Clone, Default)]
enum Blend {
	White,
	#[default]
	Black,
	Mix,
}

#[derive(Parser)]
#[command(version = CLAP_VERSION, about = "Derives an image with alpha channel from two alpha-less images")]
struct Args {
	#[arg(short, long, value_enum, help = "Which image to take the color values from (mix is experimental)", default_value_t = Blend::default())]
	blend: Blend,

	#[arg(help = "An image with a solid black background")]
	black: PathBuf,
	#[arg(help = "An image with a solid white background")]
	white: PathBuf,
	#[arg(help = "The output image")]
	out: PathBuf,
}

fn preflight_checks(black: &DynamicImage, white: &DynamicImage) -> Result<(), Error> {
	let unsupported_color_types = [ColorType::Rgb32F, ColorType::Rgba32F];
	let black_color = black.color();
	let white_color = white.color();

	if black.dimensions() != white.dimensions() {
		return Err(Error::new(
			ErrorKind::InvalidInput,
			"Both input images must be the same size",
		));
	}

	if unsupported_color_types.contains(&black_color) || unsupported_color_types.contains(&white_color) {
		return Err(Error::new(ErrorKind::InvalidInput, "32-bit color is not supported"));
	}

	if black_color != white_color {
		return Err(Error::new(
			ErrorKind::InvalidInput,
			"Both input images must use the same color format",
		));
	}

	Ok(())
}

/// Does Math™ on two input pixels from images with black and white backgrounds
/// respectively to obtain a "fixed" pixel that includes an alpha channel.
/// The input pixels are expected to be three-item f32 arrays,
/// the output pixel is a four-item f64 array.
/// Based on the method explained here: <https://www.interact-sw.co.uk/iangblog/2007/01/30/recoveralpha>
fn magic(black_pixel: [f32; 3], white_pixel: [f32; 3], blend: &Blend) -> [f64; 4] {
	let (rb, gb, bb, rw, gw, bw) = (
		f64::from(black_pixel[0]),
		f64::from(black_pixel[1]),
		f64::from(black_pixel[2]),
		f64::from(white_pixel[0]),
		f64::from(white_pixel[1]),
		f64::from(white_pixel[2]),
	);

	let (alpha, mut rs, mut gs, mut bs) = ((rb - rw + 1.0).clamp(0.0, 1.0), 0.0, 0.0, 0.0);

	if alpha > 0.0 {
		match blend {
			Blend::White => {
				rs = rw / alpha;
				gs = gw / alpha;
				bs = bw / alpha;
			}
			Blend::Black => {
				rs = rb / alpha;
				gs = gb / alpha;
				bs = bb / alpha;
			}
			Blend::Mix => {
				// not actually all that accurate, just in here as an experiment
				rs = f64::midpoint(rb, rw) / alpha;
				gs = f64::midpoint(gb, gw) / alpha;
				bs = f64::midpoint(bb, bw) / alpha;
			}
		}
	}

	[rs, gs, bs, alpha]
}

const SCALAR: f64 = 65535.0;

fn main() -> Result<(), String> {
	let args = Args::parse();

	println!("Loading images…");

	let start = Instant::now();

	let black_reader = ImageReader::open(args.black).expect("Can't open file");
	let white_reader = ImageReader::open(args.white).expect("Can't open file");

	let black_image = black_reader.decode().expect("Can't decode image");
	let white_image = white_reader.decode().expect("Can't decode image");

	preflight_checks(&black_image, &white_image).unwrap();

	let color_type = black_image.color();
	let format_name = if color_type.has_color() { "RGB" } else { "grayscale" };
	let bits_per_channel = color_type.bits_per_pixel() / u16::from(color_type.channel_count());
	let image_dim = black_image.dimensions();
	println!(
		"Generating {format_name} output at {}×{} with {bits_per_channel} bits per channel…",
		image_dim.0, image_dim.1
	);

	// Convert the input images to 32-bit RGB so we don't have to worry about integer overflow
	let black_rgb = black_image.into_rgb32f();
	let white_rgb = white_image.into_rgb32f();

	// Generate the output image in RGBA16 space, regardless of the input
	let mut out_image = ImageBuffer::new(image_dim.0, image_dim.1);

	#[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
	out_image
		.par_enumerate_pixels_mut()
		.for_each(|(x, y, pixel)| {
			let bp = black_rgb.get_pixel(x, y).0;
			let wp = white_rgb.get_pixel(x, y).0;
			let new = magic(bp, wp, &args.blend);

			*pixel = image::Rgba([
				(new[0] * SCALAR) as u16,
				(new[1] * SCALAR) as u16,
				(new[2] * SCALAR) as u16,
				(new[3] * SCALAR) as u16,
			]);
		});

	// Convert the generated image to the desired output format and save it
	match color_type {
		ColorType::L8 | ColorType::La8 => {
			let luma = ImageRgba16(out_image).into_luma_alpha8();
			luma.save_with_format(args.out.as_path(), ImageFormat::Png)
				.unwrap();
		}
		ColorType::L16 | ColorType::La16 => {
			let luma = ImageRgba16(out_image).into_luma_alpha16();
			luma.save_with_format(args.out.as_path(), ImageFormat::Png)
				.unwrap();
		}
		ColorType::Rgb8 | ColorType::Rgba8 => {
			let rgb = ImageRgba16(out_image).into_rgba8();
			rgb.save_with_format(args.out.as_path(), ImageFormat::Png)
				.unwrap();
		}
		ColorType::Rgb16 | ColorType::Rgba16 => {
			let rgb = ImageRgba16(out_image).into_rgba16();
			rgb.save_with_format(args.out.as_path(), ImageFormat::Png)
				.unwrap();
		}
		_ => {
			return Err(
				"congrats, you hit an edge case! encountering {color_type:?} here shouldn't have been possible."
					.parse()
					.unwrap(),
			);
		}
	}

	println!(
		"{} saved in {:.02}s!",
		args.out.file_name().unwrap().to_str().unwrap(),
		start.elapsed().as_secs_f64()
	);

	Ok(())
}

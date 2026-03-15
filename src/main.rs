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

static UNSUPPORTED_COLOR_TYPES: [ColorType; 2] = [ColorType::Rgb32F, ColorType::Rgba32F];
static U16_SCALAR: f32 = 65536.0;

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
	if black.dimensions() != white.dimensions() {
		return Err(Error::new(
			ErrorKind::InvalidInput,
			"Both input images must be the same size",
		));
	}

	let black_color = black.color();
	let white_color = white.color();

	if UNSUPPORTED_COLOR_TYPES.contains(&black_color) || UNSUPPORTED_COLOR_TYPES.contains(&white_color) {
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
/// the output pixel is a four-item f32 array.
/// Based on the method explained here: <https://www.interact-sw.co.uk/iangblog/2007/01/30/recoveralpha>
fn recover_alpha(black_pixel: [f32; 3], white_pixel: [f32; 3], blend: &Blend) -> [f32; 4] {
	let (rb, gb, bb, rw, gw, bw) = (
		black_pixel[0],
		black_pixel[1],
		black_pixel[2],
		white_pixel[0],
		white_pixel[1],
		white_pixel[2],
	);

	let alpha = (rb - rw + 1.0).clamp(0.0, 1.0);
	if alpha == 0.0 {
		return [0.0, 0.0, 0.0, alpha];
	}

	let (r, g, b);

	match blend {
		Blend::White => {
			r = rw / alpha;
			g = gw / alpha;
			b = bw / alpha;
		}
		Blend::Black => {
			r = rb / alpha;
			g = gb / alpha;
			b = bb / alpha;
		}
		Blend::Mix => {
			// not actually all that accurate, just in here as an experiment
			r = f32::midpoint(rb, rw) / alpha;
			g = f32::midpoint(gb, gw) / alpha;
			b = f32::midpoint(bb, bw) / alpha;
		}
	}

	[r, g, b, alpha]
}

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
	let bits_per_channel = color_type.bits_per_pixel() / u16::from(color_type.channel_count());
	let image_dim = black_image.dimensions();

	println!(
		"Generating {} output at {}×{} with {bits_per_channel} bits per channel…",
		if color_type.has_color() { "RGB" } else { "grayscale" },
		image_dim.0,
		image_dim.1
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
			let new = recover_alpha(bp, wp, &args.blend);

			*pixel = image::Rgba([
				(new[0] * U16_SCALAR).round() as u16,
				(new[1] * U16_SCALAR).round() as u16,
				(new[2] * U16_SCALAR).round() as u16,
				(new[3] * U16_SCALAR).round() as u16,
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
				"Congrats, you hit an edge case! Encountering {color_type:?} here shouldn't have been possible."
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

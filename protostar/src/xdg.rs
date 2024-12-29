use color_eyre::eyre::Result;
use freedesktop_icons_greedy::lookup;
use itertools::Itertools;
use lazy_static::lazy_static;
use regex::Regex;
use resvg::render;
use resvg::tiny_skia::{Pixmap, Transform};
use resvg::usvg::{FitTo, Tree};
use serde::{Deserialize, Serialize};
use serde_with::serde_as;
use std::collections::HashMap;
use std::ffi::OsString;
use std::fs::create_dir_all;
use std::io::{BufRead, BufReader, ErrorKind};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Mutex;
use std::{env, fs};
use walkdir::WalkDir;

#[serde_as]
#[derive(Deserialize, Serialize)]
struct ImageCache {
	path: PathBuf,
	#[serde_as(as = "Vec<(_, _)>")]
	pub map: HashMap<(String, u16), PathBuf>,
}

impl ImageCache {
	fn new(path: PathBuf) -> Self {
		if let Ok(text) = std::fs::read_to_string(&path) {
			if let Ok(cache) = toml::de::from_str(&text) {
				return cache;
			}
		}

		ImageCache {
			path,
			map: HashMap::new(),
		}
	}

	fn insert(&mut self, k: (String, u16), v: PathBuf) {
		self.map.insert(k, v);
	}

	fn save(&self) {
		std::fs::write(&self.path, toml::ser::to_string_pretty(self).unwrap()).unwrap();
	}
}

lazy_static! {
	static ref IMAGE_CACHE: Mutex<ImageCache> = Mutex::new(ImageCache::new(
		get_image_cache_dir().join("imagecache.map")
	));
}

fn get_data_dirs() -> Vec<PathBuf> {
	std::env::var("XDG_DATA_DIRS") // parse XDG_DATA_DIRS
		.unwrap_or_default()
		.split(':')
		.filter_map(|dir| PathBuf::from_str(dir).ok())
		.chain(dirs::home_dir().into_iter().map(|d| d.join(".local/share"))) // $HOME/.local/share
		.chain(PathBuf::from_str("/usr/share")) // /usr/share
		.chain(PathBuf::from_str("/usr/local/share")) // /usr/local/share
		.filter(|dir| dir.exists() && dir.is_dir())
		.unique()
		.collect()
}

fn get_app_dirs() -> Vec<PathBuf> {
	get_data_dirs()
		.into_iter()
		.map(|dir| dir.join("applications"))
		.filter(|dir| dir.exists() && dir.is_dir())
		.collect()
}

pub fn get_desktop_files() -> impl Iterator<Item = PathBuf> {
	// Get the list of directories to search
	get_app_dirs()
		.into_iter()
		.flat_map(|dir| {
			// Follow symlinks and recursively search directories
			WalkDir::new(dir)
				.follow_links(true)
				.into_iter()
				.filter_map(|entry| entry.ok())
				.filter(|entry| entry.file_type().is_file())
				.map(|entry| entry.path().to_path_buf())
		})
		.filter(|path| path.extension() == Some(&OsString::from_str("desktop").unwrap()))
}

#[test]
fn test_get_desktop_files() {
	let desktop_files = get_desktop_files().collect::<Vec<_>>();
	assert!(desktop_files
		.iter()
		.any(|file| file.ends_with("com.belmoussaoui.ashpd.demo.desktop")));
}

pub fn parse_desktop_file(path: PathBuf) -> Result<DesktopFile, String> {
	// Open the file in read-only mode
	let file = match fs::File::open(
		env::current_dir()
			.map_err(|e| e.to_string())?
			.join(path.clone()),
	) {
		Ok(file) => file,
		Err(err) => return Err(format!("Failed to open file: {}", err)),
	};

	let reader = BufReader::new(file);

	// Create temporary variables to hold the parsed values
	let mut name = None;
	let mut command = None;
	let mut categories = Vec::new();
	let mut icon = None;
	let mut no_display = false;
	let mut desktop_entry_found = false;

	let re = Regex::new(r"^\[([^\]]*)\]$").unwrap();

	// Loop through each line of the file
	for line in reader.lines() {
		let line = match line {
			Ok(line) => line,
			Err(err) => return Err(format!("Failed to read line: {}", err)),
		};

		// Skip empty lines and lines that start with "#" (comments)
		if line.is_empty() || line.starts_with('#') {
			continue;
		}

		if let Some(captures) = re.captures(&line) {
			let entry = captures.get(1).unwrap();
			desktop_entry_found = entry.as_str().contains("Desktop Entry");
		}

		if !desktop_entry_found {
			continue;
		}
		// Split the line into a key-value pair by looking for the first "=" character
		let parts = line.split_once('=');
		let (key, value) = match parts {
			Some((key, value)) => (key, value),
			None => continue,
		};

		// Parse the key-value pair based on the key
		match key {
			"Name" => name = Some(value.to_string()),
			"Exec" => command = Some(value.to_string()),
			"Categories" => {
				categories = value
					.split(';')
					.map(|s| s.to_string())
					.filter(|s| !s.is_empty())
					.collect()
			}
			"Icon" => icon = Some(value.to_string()),
			"NoDisplay" => no_display = value == "true",
			_ => (), // Ignore unknown keys
		}
	}

	// Create and return a new DesktopFile instance with the parsed values
	Ok(DesktopFile {
		path,
		name,
		command,
		categories,
		icon,
		no_display,
	})
}

#[test]
fn test_parse_desktop_file() {
	// Create a temporary directory and a test desktop file
	let dir = tempdir::TempDir::new("test").unwrap();
	let file = dir.path().join("test.desktop");
	let data = "[Desktop Entry]\nName=Test\nExec=test\nCategories=A;B;C\nIcon=test.png";
	fs::write(&file, data).unwrap();

	// Parse the test desktop file
	let desktop_file = parse_desktop_file(file).unwrap();

	// Check the parsed values
	assert_eq!(desktop_file.name, Some("Test".to_string()));
	assert_eq!(desktop_file.command, Some("test".to_string()));
	assert_eq!(
		desktop_file.categories,
		vec!["A".to_string(), "B".to_string(), "C".to_string()]
	);
	assert_eq!(desktop_file.icon, Some("test.png".to_string()));
}

#[derive(Debug, Clone)]
pub struct DesktopFile {
	path: PathBuf,
	pub name: Option<String>,
	pub command: Option<String>,
	pub categories: Vec<String>,
	pub icon: Option<String>,
	pub no_display: bool,
}

const ICON_SIZES: [u16; 7] = [512, 256, 128, 64, 48, 32, 24];

impl DesktopFile {
	pub fn get_icon(&self, preferred_px_size: u16) -> Option<Icon> {
		// Get the name of the icon from the DesktopFile struct
		let icon_name = self.icon.as_ref()?;
		let test_icon_path = self.path.join(Path::new(icon_name));
		if test_icon_path.exists() {
			if let Some(icon) = Icon::from_path(test_icon_path, preferred_px_size) {
				return Some(icon);
			}
		}

		if let Some(cache_icon_path) = IMAGE_CACHE
			.lock()
			.unwrap()
			.map
			.get(&(icon_name.clone(), preferred_px_size))
		{
			if cache_icon_path.exists() {
				if let Some(icon) = Icon::from_path(cache_icon_path.to_owned(), preferred_px_size) {
					return Some(icon);
				}
			}
		}

		let preferred_theme = match linicon_theme::get_icon_theme() {
			Some(t) => t,
			None => "hicolor".to_owned(),
		};

		if let Some(icon_path) = lookup(icon_name)
			.with_size(preferred_px_size)
			.with_theme(preferred_theme.as_str())
			.with_greed()
			.find()
		{
			if let Some(icon) = Icon::from_path(icon_path, preferred_px_size) {
				return Some(icon);
			}
		}

		for icon_size in ICON_SIZES {
			if let Some(icon_path) = lookup(icon_name)
				.with_size(icon_size)
				.with_theme(preferred_theme.as_str())
				.with_greed()
				.find()
			{
				if let Some(icon) = Icon::from_path(icon_path, preferred_px_size) {
					return Some(icon);
				}
			}
		}
		None
	}
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct Icon {
	pub icon_type: IconType,
	pub path: PathBuf,
	pub size: u16,
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum IconType {
	Png,
	Svg,
	Gltf,
}
impl Icon {
	pub fn from_path(path: PathBuf, size: u16) -> Option<Icon> {
		let icon_type = match path.extension().and_then(|ext| ext.to_str()) {
			Some("png") => IconType::Png,
			Some("svg") => IconType::Svg,
			Some("glb") | Some("gltf") => IconType::Gltf,
			_ => return None,
		};
		Some(Icon {
			icon_type,
			path,
			size,
		})
	}

	pub fn cached_process(self, size: u16) -> Result<Icon, std::io::Error> {
		let image_name = self
			.path
			.with_extension("")
			.file_name()
			.unwrap()
			.to_str()
			.unwrap()
			.to_owned();

		if !IMAGE_CACHE
			.lock()
			.unwrap()
			.map
			.contains_key(&(image_name.clone(), size))
		{
			IMAGE_CACHE
				.lock()
				.unwrap()
				.insert((image_name, size), self.path.clone());
			IMAGE_CACHE.lock().unwrap().save();
		}
		match self.icon_type {
			IconType::Svg => Ok(Icon::from_path(get_png_from_svg(self.path, size)?, size).unwrap()),
			_ => Ok(self),
		}
	}
}

#[test]
fn test_get_icon_path() {
	// Create an instance of the DesktopFile struct with some dummy data
	let desktop_file = DesktopFile {
		path: PathBuf::new(),
		name: None,
		command: None,
		categories: vec![],
		icon: Some("com.belmoussaoui.ashpd.demo".into()),
		no_display: false,
	};

	// Call the get_icon_path() function with a size argument and store the result
	let icon = desktop_file.get_icon(32);

	// Assert that the get_icon_path() function returns the expected result
	assert!(icon.is_some());
}

pub fn get_image_cache_dir() -> PathBuf {
	let cache_dir;
	if let Ok(xdg_cache_home) = std::env::var("XDG_CACHE_HOME") {
		cache_dir =
			PathBuf::from_str(&xdg_cache_home).unwrap_or(dirs::home_dir().unwrap().join(".cache"))
	} else {
		cache_dir = dirs::home_dir().unwrap().join(".cache");
	}
	let image_cache_dir = cache_dir.join("protostar_icon_cache");
	create_dir_all(&image_cache_dir).expect("Could not create image cache directory");
	image_cache_dir
}

pub fn get_png_from_svg(svg_path: impl AsRef<Path>, size: u16) -> Result<PathBuf, std::io::Error> {
	let svg_path = fs::canonicalize(svg_path)?;
	let svg_data = fs::read(svg_path.as_path())?;
	let tree = Tree::from_data(svg_data.as_slice(), &resvg::usvg::Options::default())
		.map_err(|_| ErrorKind::InvalidData)?;

	let png_path = get_image_cache_dir().join(format!(
		"{}-{}-{}.png",
		svg_path.file_name().unwrap().to_str().unwrap(),
		svg_data.len(),
		size
	));

	if png_path.exists() {
		return Ok(png_path);
	}

	let mut pixmap = Pixmap::new(size.into(), size.into()).unwrap();
	render(
		&tree,
		FitTo::Width(size.into()),
		Transform::identity(),
		pixmap.as_mut(),
	);
	pixmap
		.save_png(&png_path)
		.map_err(|_| ErrorKind::InvalidData)?;
	Ok(png_path)
}
#[test]
fn test_render_svg_to_png() {
	use image::GenericImageView;
	// Create temporary input and output paths
	let svg_path = env::current_dir().unwrap().join("test_input.svg");

	// Write some test SVG data to the input path
	let test_svg_data = "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 100 100\">
	<ellipse cx=\"50\" cy=\"80\" rx=\"46\" ry=\"19\" fill=\"#07c\"/>
	<path d=\"M43,0c-6,25,16,22,1,52c11,3,19,0,19-22c38,18,16,63-12,64c-25,2-55-39-8-94\" fill=\"#e34\"/>
	<path d=\"M34,41c-6,39,29,32,33,7c39,42-69,63-33-7\" fill=\"#fc2\"/>
</svg>";
	fs::write(&svg_path, test_svg_data).unwrap();

	// Call the function with the test input and output paths and a size of 200
	let png_path = get_png_from_svg(&svg_path, 200).unwrap();
	dbg!(&png_path);

	// Check that the output file exists
	assert!(png_path.exists());

	// Check that the output file is a PNG file
	assert_eq!(png_path.extension().unwrap(), "png");

	// Check that the output file has the expected dimensions
	let output_image = image::open(&png_path).unwrap();
	assert_eq!(output_image.dimensions(), (200, 200));

	// Delete the temporary input and output files
	fs::remove_file(&svg_path).unwrap();
	fs::remove_file(&png_path).unwrap();
}

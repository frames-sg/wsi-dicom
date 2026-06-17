use std::fs;
use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::time::Instant;

use serde::Serialize;
use signinum_j2k::{
    decode_tiles_into, encode_j2k_lossless, EncodeBackendPreference, J2kBlockCodingMode,
    J2kDecoder, J2kEncodeValidation, J2kLosslessEncodeOptions, J2kLosslessSamples, PixelFormat,
    ReversibleTransform, TileBatchOptions, TileDecodeJob,
};
use statumen::{
    Compression, DecodeExecutionOptions, PlaneSelection, Slide, SlideOpenOptions, TileCodecKind,
    TileLayout, TileRequest,
};

const DEFAULT_BATCH_SIZES: &[usize] = &[1, 16, 512, 1024];
const DEFAULT_REPEATS: usize = 3;

#[derive(Serialize)]
struct BatchMeasurement {
    batch_size: usize,
    repeats: usize,
    cold_ms: f64,
    cold_tile_ms: f64,
    sample_ms: Vec<f64>,
    median_ms: f64,
    median_tile_ms: f64,
    mean_ms: f64,
    mean_tile_ms: f64,
    min_ms: f64,
    max_ms: f64,
    tiles_per_second_median: f64,
    decoded_bytes_per_repeat: usize,
    warm_tile_ms_budget: Option<f64>,
    warm_tile_ms_budget_passed: Option<bool>,
}

#[derive(Serialize)]
struct BenchReport {
    slide_path: String,
    level: u32,
    tile_width: u32,
    tile_height: u32,
    tiles_across: u64,
    tiles_down: u64,
    available_tiles: u64,
    codec: String,
    jp2k_cpu_threads: Option<usize>,
    measurements: Vec<BatchMeasurement>,
}

#[derive(Serialize)]
struct RawBenchReport {
    raw_dir: String,
    tile_width: u32,
    tile_height: u32,
    available_tiles: usize,
    codec: String,
    jp2k_cpu_threads: Option<usize>,
    measurements: Vec<BatchMeasurement>,
}

struct RawTile {
    bytes: Vec<u8>,
    stride: usize,
    output_len: usize,
    width: u32,
    height: u32,
}

fn main() {
    if let Err(err) = run() {
        eprintln!("{err}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.is_empty() {
        return Err(
            "usage: jp2k_batch_bench <slide-path-or-raw.j2c> [batch-size ...]\n       jp2k_batch_bench --export-raw-tiles <slide-path> <output-dir> [count]\n       jp2k_batch_bench --bench-raw-tiles <raw-dir> [batch-size ...]\n       jp2k_batch_bench --generate-htj2k53 <output.j2c> [tile-size]"
                .to_string(),
        );
    }
    if args[0] == "--bench-raw-tiles" {
        return bench_raw_tiles(&args[1..]);
    }
    if args[0] == "--export-raw-tiles" {
        return export_raw_tiles(&args[1..]);
    }
    if args[0] == "--generate-htj2k53" {
        return generate_htj2k53(&args[1..]);
    }

    let slide_path = PathBuf::from(&args[0]);
    ensure_slide_path_exists(&slide_path)?;

    let batch_sizes = if args.len() > 1 {
        args[1..]
            .iter()
            .map(|value| parse_positive_usize(value, "batch size"))
            .collect::<Result<Vec<_>, _>>()?
    } else {
        DEFAULT_BATCH_SIZES.to_vec()
    };
    let repeats = bench_repeats()?;
    let warm_tile_ms_budget = warm_tile_ms_budget()?;
    let jp2k_cpu_threads = jp2k_cpu_threads()?;
    let mut decode_options = DecodeExecutionOptions::default();
    if let Some(threads) = jp2k_cpu_threads {
        let threads = NonZeroUsize::new(threads)
            .ok_or_else(|| "STATUMEN_JP2K_CPU_THREADS must be > 0".to_string())?;
        decode_options = decode_options.with_jp2k_cpu_threads(threads);
    }
    let slide = Slide::open_with_options(
        &slide_path,
        SlideOpenOptions::default().with_decode_execution_options(decode_options),
    )
    .map_err(|err| format!("open failed: {err}"))?;

    let (level, tile_width, tile_height, tiles_across, tiles_down) = select_jp2k_level(&slide)?;
    let available_tiles = tiles_across
        .checked_mul(tiles_down)
        .ok_or_else(|| "tile grid overflow".to_string())?;
    let first_req = tile_request(level, 0, 0);
    let codec = format!("{:?}", slide.tile_codec_kind(&first_req));

    let mut measurements = Vec::with_capacity(batch_sizes.len());
    for batch_size in batch_sizes {
        if batch_size as u64 > available_tiles {
            return Err(format!(
                "batch size {batch_size} exceeds available tile count {available_tiles}"
            ));
        }
        let requests = build_requests(level, tiles_across, batch_size)?;

        let cold_started = Instant::now();
        let warm = slide
            .source()
            .read_tiles_cpu(&requests)
            .map_err(|err| format!("warmup batch {batch_size} failed: {err}"))?;
        let cold_ms = cold_started.elapsed().as_secs_f64() * 1000.0;
        std::hint::black_box(decoded_bytes(&warm));

        let mut samples = Vec::with_capacity(repeats);
        let mut decoded_bytes_per_repeat = 0usize;
        for _ in 0..repeats {
            let started = Instant::now();
            let decoded = slide
                .source()
                .read_tiles_cpu(&requests)
                .map_err(|err| format!("batch {batch_size} failed: {err}"))?;
            let elapsed = started.elapsed().as_secs_f64() * 1000.0;
            decoded_bytes_per_repeat = decoded_bytes(&decoded);
            std::hint::black_box(decoded_bytes_per_repeat);
            samples.push(elapsed);
        }
        measurements.push(build_measurement(
            batch_size,
            repeats,
            cold_ms,
            samples,
            decoded_bytes_per_repeat,
            warm_tile_ms_budget,
        ));
    }

    let report = BenchReport {
        slide_path: slide_path.display().to_string(),
        level,
        tile_width,
        tile_height,
        tiles_across,
        tiles_down,
        available_tiles,
        codec,
        jp2k_cpu_threads,
        measurements,
    };
    println!(
        "{}",
        serde_json::to_string_pretty(&report).map_err(|err| err.to_string())?
    );
    enforce_budget(&report.measurements, warm_tile_ms_budget)
}

fn bench_raw_tiles(args: &[String]) -> Result<(), String> {
    if args.is_empty() {
        return Err("usage: jp2k_batch_bench --bench-raw-tiles <raw-dir> [batch-size ...]".into());
    }
    let raw_dir = PathBuf::from(&args[0]);
    if !raw_dir.is_dir() {
        return Err(format!(
            "raw tile path is not a directory: {}",
            raw_dir.display()
        ));
    }
    let batch_sizes = if args.len() > 1 {
        args[1..]
            .iter()
            .map(|value| parse_positive_usize(value, "batch size"))
            .collect::<Result<Vec<_>, _>>()?
    } else {
        DEFAULT_BATCH_SIZES.to_vec()
    };
    let repeats = bench_repeats()?;
    let warm_tile_ms_budget = warm_tile_ms_budget()?;
    let jp2k_cpu_threads = jp2k_cpu_threads()?;
    let worker_count = jp2k_cpu_threads
        .map(|threads| {
            NonZeroUsize::new(threads)
                .ok_or_else(|| "STATUMEN_JP2K_CPU_THREADS must be > 0".to_string())
        })
        .transpose()?;

    let max_batch_size = batch_sizes
        .iter()
        .copied()
        .max()
        .ok_or_else(|| "at least one batch size is required".to_string())?;
    let raw_paths = collect_raw_tile_paths(&raw_dir)?;
    if max_batch_size > raw_paths.len() {
        return Err(format!(
            "batch size {max_batch_size} exceeds available raw tile count {}",
            raw_paths.len()
        ));
    }
    let raw_tiles = raw_paths
        .iter()
        .take(max_batch_size)
        .map(|path| load_raw_tile(path))
        .collect::<Result<Vec<_>, _>>()?;
    let first_tile = raw_tiles
        .first()
        .ok_or_else(|| format!("no raw JP2K tiles found in {}", raw_dir.display()))?;

    let mut measurements = Vec::with_capacity(batch_sizes.len());
    for batch_size in batch_sizes {
        let tiles = &raw_tiles[..batch_size];
        let mut outputs = allocate_raw_outputs(tiles);
        let cold_started = Instant::now();
        decode_raw_tile_batch(tiles, &mut outputs, worker_count)?;
        let cold_ms = cold_started.elapsed().as_secs_f64() * 1000.0;
        std::hint::black_box(raw_decoded_bytes(&outputs));

        let mut samples = Vec::with_capacity(repeats);
        let mut decoded_bytes_per_repeat = 0usize;
        for _ in 0..repeats {
            clear_raw_outputs(&mut outputs);
            let started = Instant::now();
            decode_raw_tile_batch(tiles, &mut outputs, worker_count)?;
            let elapsed = started.elapsed().as_secs_f64() * 1000.0;
            decoded_bytes_per_repeat = raw_decoded_bytes(&outputs);
            std::hint::black_box(decoded_bytes_per_repeat);
            samples.push(elapsed);
        }
        measurements.push(build_measurement(
            batch_size,
            repeats,
            cold_ms,
            samples,
            decoded_bytes_per_repeat,
            warm_tile_ms_budget,
        ));
    }

    let report = RawBenchReport {
        raw_dir: raw_dir.display().to_string(),
        tile_width: first_tile.width,
        tile_height: first_tile.height,
        available_tiles: raw_paths.len(),
        codec: "raw-jp2k".into(),
        jp2k_cpu_threads,
        measurements,
    };
    println!(
        "{}",
        serde_json::to_string_pretty(&report).map_err(|err| err.to_string())?
    );
    enforce_budget(&report.measurements, warm_tile_ms_budget)?;
    Ok(())
}

fn generate_htj2k53(args: &[String]) -> Result<(), String> {
    if args.is_empty() || args.len() > 2 {
        return Err("usage: jp2k_batch_bench --generate-htj2k53 <output.j2c> [tile-size]".into());
    }
    let output_path = PathBuf::from(&args[0]);
    let tile_size = args
        .get(1)
        .map(|value| parse_positive_u32(value, "tile size"))
        .transpose()?
        .unwrap_or(512);
    let pixels = synthetic_rgb_tile(tile_size, tile_size)?;
    let samples = J2kLosslessSamples::new(&pixels, tile_size, tile_size, 3, 8, false)
        .map_err(|err| format!("build synthetic HTJ2K samples: {err}"))?;
    let encoded = encode_j2k_lossless(
        samples,
        &J2kLosslessEncodeOptions {
            backend: EncodeBackendPreference::CpuOnly,
            block_coding_mode: J2kBlockCodingMode::HighThroughput,
            reversible_transform: ReversibleTransform::Rct53,
            validation: J2kEncodeValidation::External,
            ..J2kLosslessEncodeOptions::default()
        },
    )
    .map_err(|err| format!("encode synthetic HTJ2K 5/3 tile: {err}"))?;
    fs::write(&output_path, &encoded.codestream)
        .map_err(|err| format!("write {}: {err}", output_path.display()))?;
    println!(
        "{}",
        serde_json::json!({
            "output_path": output_path.display().to_string(),
            "tile_width": tile_size,
            "tile_height": tile_size,
            "components": 3,
            "bit_depth": 8,
            "transform": "reversible_5_3",
            "block_coding": "htj2k",
            "bytes": encoded.codestream.len(),
        })
    );
    Ok(())
}

fn export_raw_tiles(args: &[String]) -> Result<(), String> {
    if args.len() < 2 || args.len() > 3 {
        return Err(
            "usage: jp2k_batch_bench --export-raw-tiles <slide-path> <output-dir> [count]"
                .to_string(),
        );
    }
    let slide_path = PathBuf::from(&args[0]);
    ensure_slide_path_exists(&slide_path)?;
    let output_dir = PathBuf::from(&args[1]);
    let requested_count = args
        .get(2)
        .map(|value| parse_positive_usize(value, "export count"))
        .transpose()?;

    fs::create_dir_all(&output_dir)
        .map_err(|err| format!("create output dir {}: {err}", output_dir.display()))?;

    let slide = Slide::open_with_options(
        &slide_path,
        SlideOpenOptions::default()
            .with_decode_execution_options(DecodeExecutionOptions::default()),
    )
    .map_err(|err| format!("open failed: {err}"))?;
    let (level, tile_width, tile_height, tiles_across, tiles_down) = select_jp2k_level(&slide)?;
    let available_tiles = tiles_across
        .checked_mul(tiles_down)
        .ok_or_else(|| "tile grid overflow".to_string())?;
    let count = requested_count
        .unwrap_or(usize::try_from(available_tiles).map_err(|_| "tile count overflow")?);
    if count as u64 > available_tiles {
        return Err(format!(
            "export count {count} exceeds available tile count {available_tiles}"
        ));
    }

    let requests = build_requests(level, tiles_across, count)?;
    let mut skipped = 0usize;
    let mut exported = 0usize;
    for (index, req) in requests.iter().enumerate() {
        let raw = slide
            .read_raw_compressed_tile(req)
            .map_err(|err| format!("read raw tile {index}: {err}"))?;
        let extension = match raw.compression {
            Compression::Jp2kRgb | Compression::Jp2kYcbcr => "j2k",
            _ => {
                skipped += 1;
                continue;
            }
        };
        let path = output_dir.join(format!(
            "tile_{index:06}_r{row:05}_c{col:05}.{extension}",
            row = req.row,
            col = req.col
        ));
        fs::write(&path, raw.data)
            .map_err(|err| format!("write raw tile {}: {err}", path.display()))?;
        exported += 1;
    }

    println!(
        "{}",
        serde_json::json!({
            "slide_path": slide_path.display().to_string(),
            "output_dir": output_dir.display().to_string(),
            "level": level,
            "tile_width": tile_width,
            "tile_height": tile_height,
            "tiles_across": tiles_across,
            "tiles_down": tiles_down,
            "available_tiles": available_tiles,
            "requested_tiles": count,
            "exported_tiles": exported,
            "skipped_non_jp2k_tiles": skipped,
        })
    );
    Ok(())
}

fn bench_repeats() -> Result<usize, String> {
    std::env::var("STATUMEN_JP2K_BATCH_BENCH_REPEATS")
        .ok()
        .map(|value| parse_positive_usize(&value, "STATUMEN_JP2K_BATCH_BENCH_REPEATS"))
        .transpose()
        .map(|value| value.unwrap_or(DEFAULT_REPEATS))
}

fn warm_tile_ms_budget() -> Result<Option<f64>, String> {
    std::env::var("STATUMEN_JP2K_BATCH_BENCH_MAX_WARM_TILE_MS")
        .ok()
        .map(|value| parse_positive_f64(&value, "STATUMEN_JP2K_BATCH_BENCH_MAX_WARM_TILE_MS"))
        .transpose()
}

fn jp2k_cpu_threads() -> Result<Option<usize>, String> {
    std::env::var("STATUMEN_JP2K_CPU_THREADS")
        .ok()
        .map(|value| parse_positive_usize(&value, "STATUMEN_JP2K_CPU_THREADS"))
        .transpose()
}

fn select_jp2k_level(slide: &Slide) -> Result<(u32, u32, u32, u64, u64), String> {
    let series = &slide.dataset().scenes[0].series[0];
    for (level_index, level) in series.levels.iter().enumerate() {
        let (tile_width, tile_height, tiles_across, tiles_down) = match &level.tile_layout {
            TileLayout::Regular {
                tile_width,
                tile_height,
                tiles_across,
                tiles_down,
            } => (*tile_width, *tile_height, *tiles_across, *tiles_down),
            _ => continue,
        };
        let level = u32::try_from(level_index).map_err(|_| "level index overflow".to_string())?;
        let codec = slide.tile_codec_kind(&tile_request(level, 0, 0));
        if matches!(codec, TileCodecKind::Jp2k | TileCodecKind::Htj2k) {
            return Ok((level, tile_width, tile_height, tiles_across, tiles_down));
        }
    }
    Err("no regular JP2K/HTJ2K level found".into())
}

fn collect_raw_tile_paths(raw_dir: &std::path::Path) -> Result<Vec<PathBuf>, String> {
    let mut paths = fs::read_dir(raw_dir)
        .map_err(|err| format!("read raw tile dir {}: {err}", raw_dir.display()))?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<std::io::Result<Vec<_>>>()
        .map_err(|err| format!("read raw tile dir {}: {err}", raw_dir.display()))?;
    paths.retain(|path| {
        path.extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| matches!(ext.to_ascii_lowercase().as_str(), "j2k" | "j2c"))
    });
    paths.sort();
    if paths.is_empty() {
        return Err(format!("no .j2k/.j2c tiles found in {}", raw_dir.display()));
    }
    Ok(paths)
}

fn load_raw_tile(path: &std::path::Path) -> Result<RawTile, String> {
    let bytes = fs::read(path).map_err(|err| format!("read raw tile {}: {err}", path.display()))?;
    let decoder = J2kDecoder::new(&bytes)
        .map_err(|err| format!("inspect raw tile {}: {err}", path.display()))?;
    let (width, height) = decoder.info().dimensions;
    let stride = (width as usize)
        .checked_mul(3)
        .ok_or_else(|| format!("raw tile {} row stride overflow", path.display()))?;
    let output_len = stride
        .checked_mul(height as usize)
        .ok_or_else(|| format!("raw tile {} output size overflow", path.display()))?;
    Ok(RawTile {
        bytes,
        stride,
        output_len,
        width,
        height,
    })
}

fn allocate_raw_outputs(tiles: &[RawTile]) -> Vec<Vec<u8>> {
    tiles
        .iter()
        .map(|tile| vec![0_u8; tile.output_len])
        .collect()
}

fn clear_raw_outputs(outputs: &mut [Vec<u8>]) {
    for output in outputs {
        output.fill(0);
    }
}

fn decode_raw_tile_batch(
    tiles: &[RawTile],
    outputs: &mut [Vec<u8>],
    worker_count: Option<NonZeroUsize>,
) -> Result<(), String> {
    let mut jobs = tiles
        .iter()
        .zip(outputs.iter_mut())
        .map(|(tile, output)| TileDecodeJob {
            input: tile.bytes.as_slice(),
            out: output.as_mut_slice(),
            stride: tile.stride,
        })
        .collect::<Vec<_>>();
    decode_tiles_into(
        &mut jobs,
        PixelFormat::Rgb8,
        TileBatchOptions {
            workers: worker_count,
        },
    )
    .map_err(|err| format!("raw tile batch decode failed: {err}"))?;
    Ok(())
}

fn raw_decoded_bytes(outputs: &[Vec<u8>]) -> usize {
    outputs.iter().map(Vec::len).sum()
}

fn build_measurement(
    batch_size: usize,
    repeats: usize,
    cold_ms: f64,
    samples: Vec<f64>,
    decoded_bytes_per_repeat: usize,
    warm_tile_ms_budget: Option<f64>,
) -> BatchMeasurement {
    let median_ms = median(samples.clone());
    let mean_ms = samples.iter().sum::<f64>() / samples.len() as f64;
    let min_ms = samples.iter().copied().fold(f64::INFINITY, f64::min);
    let max_ms = samples.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let median_tile_ms = per_tile_ms(median_ms, batch_size);
    let warm_tile_ms_budget_passed = warm_tile_ms_budget.map(|budget| median_tile_ms <= budget);
    BatchMeasurement {
        batch_size,
        repeats,
        cold_ms,
        cold_tile_ms: per_tile_ms(cold_ms, batch_size),
        sample_ms: samples,
        median_ms,
        median_tile_ms,
        mean_ms,
        mean_tile_ms: per_tile_ms(mean_ms, batch_size),
        min_ms,
        max_ms,
        tiles_per_second_median: batch_size as f64 / (median_ms / 1000.0),
        decoded_bytes_per_repeat,
        warm_tile_ms_budget,
        warm_tile_ms_budget_passed,
    }
}

fn enforce_budget(
    measurements: &[BatchMeasurement],
    warm_tile_ms_budget: Option<f64>,
) -> Result<(), String> {
    if let Some(budget) = warm_tile_ms_budget {
        if let Some(failed) = measurements
            .iter()
            .find(|measurement| measurement.median_tile_ms > budget)
        {
            return Err(format!(
                "warm median guard failed for batch {}: {:.3} ms/tile > {:.3} ms/tile",
                failed.batch_size, failed.median_tile_ms, budget
            ));
        }
    }
    Ok(())
}

fn ensure_slide_path_exists(path: &std::path::Path) -> Result<(), String> {
    if path.is_file() || path.is_dir() {
        return Ok(());
    }
    Err(format!(
        "slide path is not a file or directory: {}",
        path.display()
    ))
}

fn build_requests(
    level: u32,
    tiles_across: u64,
    batch_size: usize,
) -> Result<Vec<TileRequest>, String> {
    (0..batch_size)
        .map(|index| {
            let index = u64::try_from(index).map_err(|_| "batch index overflow".to_string())?;
            let col = i64::try_from(index % tiles_across)
                .map_err(|_| "tile column overflow".to_string())?;
            let row =
                i64::try_from(index / tiles_across).map_err(|_| "tile row overflow".to_string())?;
            Ok(tile_request(level, col, row))
        })
        .collect()
}

fn tile_request(level: u32, col: i64, row: i64) -> TileRequest {
    TileRequest {
        scene: 0,
        series: 0,
        level,
        plane: PlaneSelection::default(),
        col,
        row,
    }
}

fn decoded_bytes(tiles: &[statumen::CpuTile]) -> usize {
    tiles.iter().map(|tile| tile.data.byte_size()).sum()
}

fn parse_positive_usize(value: &str, label: &str) -> Result<usize, String> {
    let parsed = value
        .parse::<usize>()
        .map_err(|err| format!("invalid {label} {value:?}: {err}"))?;
    if parsed == 0 {
        return Err(format!("{label} must be > 0"));
    }
    Ok(parsed)
}

fn parse_positive_u32(value: &str, label: &str) -> Result<u32, String> {
    let parsed = value
        .parse::<u32>()
        .map_err(|err| format!("invalid {label} {value:?}: {err}"))?;
    if parsed == 0 {
        return Err(format!("{label} must be > 0"));
    }
    Ok(parsed)
}

fn parse_positive_f64(value: &str, label: &str) -> Result<f64, String> {
    let parsed = value
        .parse::<f64>()
        .map_err(|err| format!("invalid {label} {value:?}: {err}"))?;
    if !parsed.is_finite() || parsed <= 0.0 {
        return Err(format!("{label} must be finite and > 0"));
    }
    Ok(parsed)
}

fn median(mut samples: Vec<f64>) -> f64 {
    samples.sort_by(|a, b| a.total_cmp(b));
    samples[samples.len() / 2]
}

fn per_tile_ms(elapsed_ms: f64, batch_size: usize) -> f64 {
    elapsed_ms / batch_size as f64
}

fn synthetic_rgb_tile(width: u32, height: u32) -> Result<Vec<u8>, String> {
    let len = (width as usize)
        .checked_mul(height as usize)
        .and_then(|pixels| pixels.checked_mul(3))
        .ok_or_else(|| "synthetic tile size overflow".to_string())?;
    let mut pixels = Vec::with_capacity(len);
    for y in 0..height {
        for x in 0..width {
            pixels.push(((x.wrapping_mul(13) ^ y.wrapping_mul(7)) & 0xff) as u8);
            pixels.push(((x.wrapping_mul(3).wrapping_add(y.wrapping_mul(17))) & 0xff) as u8);
            pixels.push(((x.wrapping_mul(11).wrapping_add(y.wrapping_mul(5))) & 0xff) as u8);
        }
    }
    Ok(pixels)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn per_tile_ms_divides_elapsed_by_batch_size() {
        assert_eq!(per_tile_ms(100.0, 4), 25.0);
    }

    #[test]
    fn parse_positive_f64_rejects_non_positive_and_non_finite_values() {
        assert!(parse_positive_f64("25", "budget").is_ok());
        assert!(parse_positive_f64("0", "budget").is_err());
        assert!(parse_positive_f64("-1", "budget").is_err());
        assert!(parse_positive_f64("inf", "budget").is_err());
    }

    #[test]
    fn synthetic_rgb_tile_has_expected_size_and_nonconstant_channels() {
        let pixels = synthetic_rgb_tile(8, 8).expect("synthetic pixels");
        assert_eq!(pixels.len(), 8 * 8 * 3);
        assert!(pixels.windows(2).any(|window| window[0] != window[1]));
    }

    #[test]
    fn ensure_slide_path_accepts_directory() {
        let dir = tempfile::tempdir().expect("tempdir");

        assert!(ensure_slide_path_exists(dir.path()).is_ok());
    }
}

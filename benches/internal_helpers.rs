use std::hint::black_box;
use std::path::Path;

use criterion::{criterion_group, criterion_main, BatchSize, Criterion, Throughput};
use wsi_dicom::bench_support::{
    instance_context_summary, pixel_data_offsets_for_bench, prepare_tile_samples_summary,
};
use wsi_rs::{ColorSpace, CpuTile, CpuTileData, CpuTileLayout};

fn rgb8_tile(width: u32, height: u32) -> CpuTile {
    let mut data = Vec::with_capacity(width as usize * height as usize * 3);
    for y in 0..height {
        for x in 0..width {
            data.push((x.wrapping_mul(37) ^ y.wrapping_mul(11)) as u8);
            data.push((x.wrapping_mul(17) ^ y.wrapping_mul(29)) as u8);
            data.push((x.wrapping_mul(7) ^ y.wrapping_mul(43)) as u8);
        }
    }
    CpuTile {
        width,
        height,
        channels: 3,
        color_space: ColorSpace::Rgb,
        layout: CpuTileLayout::Interleaved,
        data: CpuTileData::u8(data),
    }
}

fn bench_prepare_tile_samples(c: &mut Criterion) {
    let mut group = c.benchmark_group("prepare_tile_samples");
    group.throughput(Throughput::Bytes(512 * 512 * 3));
    let tile_512 = rgb8_tile(512, 512);
    group.bench_function("rgb8_512_exact", |b| {
        b.iter(|| {
            black_box(
                prepare_tile_samples_summary(black_box(&tile_512), black_box(512), black_box(512))
                    .expect("prepare exact RGB tile"),
            );
        });
    });

    group.throughput(Throughput::Bytes(510 * 509 * 3));
    let edge_tile = rgb8_tile(510, 509);
    group.bench_function("rgb8_edge_pad_to_512", |b| {
        b.iter(|| {
            black_box(
                prepare_tile_samples_summary(black_box(&edge_tile), black_box(512), black_box(512))
                    .expect("prepare padded RGB tile"),
            );
        });
    });
    group.finish();
}

fn bench_pixel_data_offsets(c: &mut Criterion) {
    let lengths = (0..16_384)
        .map(|idx| 997 + (idx % 251) as u64)
        .collect::<Vec<_>>();
    c.bench_function("pixel_data_offsets/16k_frames", |b| {
        b.iter(|| {
            black_box(
                pixel_data_offsets_for_bench(black_box(&lengths))
                    .expect("compute pixel data offsets"),
            );
        });
    });
}

fn bench_instance_context(c: &mut Criterion) {
    let source_path = Path::new("/bench/source/slide.svs");
    let output_dir = Path::new("/bench/output");
    c.bench_function("instance_context/new_and_report", |b| {
        b.iter_batched(
            || (source_path, output_dir),
            |(source_path, output_dir)| {
                black_box(instance_context_summary(
                    black_box(source_path),
                    black_box(output_dir),
                    black_box(1),
                    black_box(2),
                    black_box(3),
                    black_box(4),
                    black_box(5),
                    black_box(6),
                ));
            },
            BatchSize::SmallInput,
        );
    });
}

criterion_group!(
    benches,
    bench_prepare_tile_samples,
    bench_pixel_data_offsets,
    bench_instance_context
);
criterion_main!(benches);

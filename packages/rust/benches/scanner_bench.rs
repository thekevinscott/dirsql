use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use dirsql::matcher::TableMatcher;
use dirsql::scanner::scan_directory;
use std::fs;
use tempfile::TempDir;

fn create_temp_tree(file_count: usize) -> TempDir {
    let dir = TempDir::new().unwrap();
    for i in 0..file_count {
        // Distribute files across subdirectories
        let subdir = dir.path().join(format!("dir_{}", i % 10));
        fs::create_dir_all(&subdir).unwrap();
        fs::write(
            subdir.join(format!("file_{i}.csv")),
            format!("id,val\n{i},x"),
        )
        .unwrap();
    }
    // Add some non-matching files
    for i in 0..file_count / 5 {
        fs::write(dir.path().join(format!("readme_{i}.md")), "# not matched").unwrap();
    }
    dir
}

fn bench_scan_directory(c: &mut Criterion) {
    let mut group = c.benchmark_group("scanner/scan_directory");
    for count in [10, 100, 1000] {
        let dir = create_temp_tree(count);
        let matcher = TableMatcher::new(&[("**/*.csv", "data")], &[]).unwrap();
        group.bench_with_input(BenchmarkId::from_parameter(count), &count, |b, _| {
            b.iter(|| {
                scan_directory(dir.path(), &matcher);
            });
        });
    }
    group.finish();
}

fn bench_scan_with_ignores(c: &mut Criterion) {
    let dir = create_temp_tree(500);
    let matcher = TableMatcher::new(
        &[("**/*.csv", "data"), ("**/*.md", "docs")],
        &["**/dir_0/**", "**/dir_1/**", "**/dir_2/**"],
    )
    .unwrap();
    c.bench_function("scanner/scan_with_ignores_500", |b| {
        b.iter(|| {
            scan_directory(dir.path(), &matcher);
        });
    });
}

criterion_group!(benches, bench_scan_directory, bench_scan_with_ignores);
criterion_main!(benches);

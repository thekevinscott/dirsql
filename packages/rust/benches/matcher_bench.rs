use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use dirsql::matcher::TableMatcher;
use std::path::Path;

fn make_matcher(pattern_count: usize) -> TableMatcher {
    let patterns: Vec<(String, String)> = (0..pattern_count)
        .map(|i| (format!("**/*.ext{i}"), format!("table_{i}")))
        .collect();
    let refs: Vec<(&str, &str)> = patterns
        .iter()
        .map(|(p, t)| (p.as_str(), t.as_str()))
        .collect();
    TableMatcher::new(&refs, &[]).unwrap()
}

fn make_test_paths(count: usize) -> Vec<String> {
    (0..count)
        .map(|i| format!("project/src/dir_{}/file_{}.ext{}", i % 20, i, i % 50))
        .collect()
}

fn bench_match_file(c: &mut Criterion) {
    let mut group = c.benchmark_group("matcher/match_file");
    for pattern_count in [1, 10, 50] {
        let matcher = make_matcher(pattern_count);
        let paths = make_test_paths(100);
        group.bench_with_input(
            BenchmarkId::from_parameter(format!("{pattern_count}_patterns")),
            &pattern_count,
            |b, _| {
                b.iter(|| {
                    for p in &paths {
                        matcher.match_file(Path::new(p));
                    }
                });
            },
        );
    }
    group.finish();
}

fn bench_is_ignored(c: &mut Criterion) {
    let ignore_patterns: Vec<String> = (0..20).map(|i| format!("**/dir_{i}/**")).collect();
    let ignore_refs: Vec<&str> = ignore_patterns.iter().map(|s| s.as_str()).collect();
    let matcher = TableMatcher::new(&[("**/*.csv", "t")], &ignore_refs).unwrap();
    let paths = make_test_paths(100);

    c.bench_function("matcher/is_ignored_20_patterns", |b| {
        b.iter(|| {
            for p in &paths {
                matcher.is_ignored(Path::new(p));
            }
        });
    });
}

fn bench_match_construction(c: &mut Criterion) {
    let mut group = c.benchmark_group("matcher/construction");
    for pattern_count in [1, 10, 50] {
        let patterns: Vec<(String, String)> = (0..pattern_count)
            .map(|i| (format!("**/*.ext{i}"), format!("table_{i}")))
            .collect();
        let refs: Vec<(&str, &str)> = patterns
            .iter()
            .map(|(p, t)| (p.as_str(), t.as_str()))
            .collect();
        group.bench_with_input(
            BenchmarkId::from_parameter(pattern_count),
            &pattern_count,
            |b, _| {
                b.iter(|| {
                    TableMatcher::new(&refs, &[]).unwrap();
                });
            },
        );
    }
    group.finish();
}

criterion_group!(
    benches,
    bench_match_file,
    bench_is_ignored,
    bench_match_construction
);
criterion_main!(benches);

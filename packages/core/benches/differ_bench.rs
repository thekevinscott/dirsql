use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use dirsql_core::db::Value;
use dirsql_core::differ::diff;
use std::collections::HashMap;

fn make_rows(count: usize) -> Vec<HashMap<String, Value>> {
    (0..count)
        .map(|i| {
            HashMap::from([
                ("id".to_string(), Value::Integer(i as i64)),
                ("name".to_string(), Value::Text(format!("item_{i}"))),
                ("value".to_string(), Value::Real(i as f64)),
            ])
        })
        .collect()
}

fn bench_diff_new_file(c: &mut Criterion) {
    let mut group = c.benchmark_group("differ/new_file");
    for count in [10, 100, 1000] {
        let rows = make_rows(count);
        group.bench_with_input(BenchmarkId::from_parameter(count), &count, |b, _| {
            b.iter(|| {
                diff("t", None, Some(&rows), "f.jsonl");
            });
        });
    }
    group.finish();
}

fn bench_diff_no_change(c: &mut Criterion) {
    let mut group = c.benchmark_group("differ/no_change");
    for count in [10, 100, 1000] {
        let rows = make_rows(count);
        group.bench_with_input(BenchmarkId::from_parameter(count), &count, |b, _| {
            b.iter(|| {
                diff("t", Some(&rows), Some(&rows), "f.jsonl");
            });
        });
    }
    group.finish();
}

fn bench_diff_single_line_change(c: &mut Criterion) {
    let mut group = c.benchmark_group("differ/single_line_change");
    for count in [10, 100, 1000] {
        let old = make_rows(count);
        let mut new = old.clone();
        // Change one row in the middle
        new[count / 2].insert("name".to_string(), Value::Text("CHANGED".to_string()));
        group.bench_with_input(BenchmarkId::from_parameter(count), &count, |b, _| {
            b.iter(|| {
                diff("t", Some(&old), Some(&new), "f.jsonl");
            });
        });
    }
    group.finish();
}

fn bench_diff_append(c: &mut Criterion) {
    let mut group = c.benchmark_group("differ/append");
    for count in [10, 100, 1000] {
        let old = make_rows(count);
        let mut new = old.clone();
        // Append 10% more rows
        let extra = make_rows(count + count / 10);
        new.extend_from_slice(&extra[count..]);
        group.bench_with_input(BenchmarkId::from_parameter(count), &count, |b, _| {
            b.iter(|| {
                diff("t", Some(&old), Some(&new), "f.jsonl");
            });
        });
    }
    group.finish();
}

fn bench_diff_full_replace(c: &mut Criterion) {
    let mut group = c.benchmark_group("differ/full_replace");
    for count in [10, 100, 1000] {
        let old = make_rows(count);
        // Completely different rows trigger full replace
        let new: Vec<HashMap<String, Value>> = (0..count)
            .map(|i| {
                HashMap::from([
                    ("id".to_string(), Value::Integer((i + count) as i64)),
                    ("name".to_string(), Value::Text(format!("new_{i}"))),
                    ("value".to_string(), Value::Real(i as f64 * 2.0)),
                ])
            })
            .collect();
        group.bench_with_input(BenchmarkId::from_parameter(count), &count, |b, _| {
            b.iter(|| {
                diff("t", Some(&old), Some(&new), "f.jsonl");
            });
        });
    }
    group.finish();
}

criterion_group!(
    benches,
    bench_diff_new_file,
    bench_diff_no_change,
    bench_diff_single_line_change,
    bench_diff_append,
    bench_diff_full_replace
);
criterion_main!(benches);

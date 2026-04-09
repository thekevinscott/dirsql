use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use dirsql_core::db::{Db, Value};
use std::collections::HashMap;

fn make_row(i: usize) -> HashMap<String, Value> {
    HashMap::from([
        ("id".to_string(), Value::Integer(i as i64)),
        ("name".to_string(), Value::Text(format!("item_{i}"))),
        ("score".to_string(), Value::Real(i as f64 * 1.5)),
    ])
}

fn bench_create_table(c: &mut Criterion) {
    c.bench_function("db/create_table", |b| {
        b.iter(|| {
            let db = Db::new().unwrap();
            db.create_table("CREATE TABLE items (id INTEGER, name TEXT, score REAL)")
                .unwrap();
        });
    });
}

fn bench_insert_rows(c: &mut Criterion) {
    let mut group = c.benchmark_group("db/insert_rows");
    for count in [1, 100, 1000] {
        group.bench_with_input(BenchmarkId::from_parameter(count), &count, |b, &n| {
            b.iter(|| {
                let db = Db::new().unwrap();
                db.create_table("CREATE TABLE items (id INTEGER, name TEXT, score REAL)")
                    .unwrap();
                for i in 0..n {
                    db.insert_row("items", &make_row(i), "data.jsonl", i)
                        .unwrap();
                }
            });
        });
    }
    group.finish();
}

fn bench_query(c: &mut Criterion) {
    let mut group = c.benchmark_group("db/query");
    for count in [100, 1000] {
        group.bench_with_input(BenchmarkId::from_parameter(count), &count, |b, &n| {
            let db = Db::new().unwrap();
            db.create_table("CREATE TABLE items (id INTEGER, name TEXT, score REAL)")
                .unwrap();
            for i in 0..n {
                db.insert_row("items", &make_row(i), "data.jsonl", i)
                    .unwrap();
            }
            b.iter(|| {
                db.query("SELECT id, name, score FROM items WHERE score > 50.0")
                    .unwrap();
            });
        });
    }
    group.finish();
}

fn bench_delete_by_file(c: &mut Criterion) {
    c.bench_function("db/delete_by_file", |b| {
        b.iter_with_setup(
            || {
                let db = Db::new().unwrap();
                db.create_table("CREATE TABLE items (id INTEGER, name TEXT, score REAL)")
                    .unwrap();
                for i in 0..500 {
                    let file = if i % 2 == 0 { "a.jsonl" } else { "b.jsonl" };
                    db.insert_row("items", &make_row(i), file, i).unwrap();
                }
                db
            },
            |db| {
                db.delete_rows_by_file("items", "a.jsonl").unwrap();
            },
        );
    });
}

criterion_group!(
    benches,
    bench_create_table,
    bench_insert_rows,
    bench_query,
    bench_delete_by_file
);
criterion_main!(benches);

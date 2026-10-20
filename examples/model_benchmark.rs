//! Benchmark the production Rust model loader and weighted inference path.
//!
//! Run with `cargo run --release --example model_benchmark -- --input cases.json
//! --output rust-results.json --repeats 9 --warmup 2`.
//! Set RAYON_NUM_THREADS before launching to select a fixed worker count.

use std::{env, fs, hint::black_box, path::PathBuf, process, time::Instant};

use serde::{Deserialize, Serialize};

#[path = "../src/domain/mlp.rs"]
pub mod mlp;

// model_store.rs uses the same domain::mlp path as the application.
mod domain {
    pub use crate::mlp;
}

#[allow(dead_code)]
#[path = "../src/domain/model_store.rs"]
mod model_store;

use model_store::EmbeddedModelStore;

#[derive(Deserialize)]
struct BenchmarkInput {
    cases: Vec<InputCase>,
}

#[derive(Deserialize)]
struct InputCase {
    name: String,
    building_type: String,
    inputs: Vec<Vec<f32>>,
}

#[derive(Serialize)]
struct BenchmarkOutput {
    load_ms: f64,
    rayon_threads: usize,
    model_count: usize,
    total_params: usize,
    repeats: usize,
    warmup: usize,
    cases: Vec<CaseOutput>,
}

#[derive(Serialize)]
struct CaseOutput {
    name: String,
    building_type: String,
    samples: usize,
    times_ms: Vec<f64>,
    median_ms: f64,
    min_ms: f64,
    max_ms: f64,
    outputs: Vec<Vec<f32>>,
}

struct Options {
    input: PathBuf,
    output: PathBuf,
    repeats: usize,
    warmup: usize,
}

fn options() -> Result<Options, String> {
    let mut args = env::args().skip(1);
    let mut input = None;
    let mut output = None;
    let mut repeats = 9;
    let mut warmup = 2;
    while let Some(arg) = args.next() {
        if arg == "--help" || arg == "-h" {
            println!(
                "model_benchmark --input PATH --output PATH [--repeats N] [--warmup N]\n\
                 Defaults: --repeats 9 --warmup 2. Use RAYON_NUM_THREADS to fix worker count."
            );
            process::exit(0);
        }
        let value = match arg.as_str() {
            "--input" | "--output" | "--repeats" | "--warmup" => args
                .next()
                .ok_or_else(|| format!("missing value for {arg}"))?,
            _ => return Err(format!("unknown argument: {arg}")),
        };
        match arg.as_str() {
            "--input" => input = Some(PathBuf::from(value)),
            "--output" => output = Some(PathBuf::from(value)),
            "--repeats" => {
                repeats = value
                    .parse()
                    .map_err(|_| "--repeats must be a positive integer".to_string())?;
            }
            "--warmup" => {
                warmup = value
                    .parse()
                    .map_err(|_| "--warmup must be a nonnegative integer".to_string())?;
            }
            _ => unreachable!(),
        }
    }
    if repeats == 0 {
        return Err("--repeats must be greater than zero".to_string());
    }
    Ok(Options {
        input: input.ok_or("--input is required")?,
        output: output.ok_or("--output is required")?,
        repeats,
        warmup,
    })
}

fn run() -> Result<(), String> {
    let options = options()?;
    let input_bytes = fs::read(&options.input).map_err(|error| error.to_string())?;
    let input: BenchmarkInput =
        serde_json::from_slice(&input_bytes).map_err(|error| error.to_string())?;
    if input.cases.is_empty() {
        return Err("input must contain at least one case".to_string());
    }

    let start = Instant::now();
    let store = EmbeddedModelStore::load()?;
    let load_ms = start.elapsed().as_secs_f64() * 1_000.0;

    // Initialize the global thread pool outside the timed inference sections.
    let rayon_threads = rayon::current_num_threads();
    let mut cases = Vec::with_capacity(input.cases.len());
    for case in input.cases {
        if case.inputs.is_empty() {
            return Err(format!("case {} has no input samples", case.name));
        }
        for _ in 0..options.warmup {
            black_box(store.predict_weighted_segments(&case.building_type, &case.inputs)?);
        }

        let mut times_ms = Vec::with_capacity(options.repeats);
        let mut outputs = Vec::new();
        for _ in 0..options.repeats {
            let start = Instant::now();
            let predicted = store.predict_weighted_segments(
                black_box(&case.building_type),
                black_box(&case.inputs),
            )?;
            let elapsed_ms = start.elapsed().as_secs_f64() * 1_000.0;
            times_ms.push(elapsed_ms);
            // Previous output disposal and result serialization are outside timing.
            outputs = black_box(predicted);
        }
        let mut sorted = times_ms.clone();
        sorted.sort_by(f64::total_cmp);
        let middle = sorted.len() / 2;
        let median_ms = if sorted.len() % 2 == 0 {
            (sorted[middle - 1] + sorted[middle]) / 2.0
        } else {
            sorted[middle]
        };
        cases.push(CaseOutput {
            name: case.name,
            building_type: case.building_type,
            samples: case.inputs.len(),
            median_ms,
            min_ms: sorted[0],
            max_ms: sorted[sorted.len() - 1],
            times_ms,
            outputs,
        });
    }
    let result = BenchmarkOutput {
        load_ms,
        rayon_threads,
        model_count: store.len(),
        total_params: store.total_params(),
        repeats: options.repeats,
        warmup: options.warmup,
        cases,
    };
    let json = serde_json::to_vec_pretty(&result).map_err(|error| error.to_string())?;
    fs::write(&options.output, json).map_err(|error| error.to_string())?;
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("model_benchmark: {error}");
        process::exit(1);
    }
}

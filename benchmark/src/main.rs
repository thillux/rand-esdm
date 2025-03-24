use rand_chacha::ChaCha20Rng;
use rand_core::{OsRng, RngCore, SeedableRng, TryRngCore};
use rand_esdm::{EsdmRng, EsdmRngType};

trait Benchmark {
    fn fill_bytes(&mut self, dst: &mut [u8]);
}

fn benchmark_rng(rng: &mut impl Benchmark) {
    use std::time::Instant;

    let sizes = cute::c![1 << x, for x in 0..15];
    let iterations = 20000;

    for size in &sizes {
        let mut buf = vec![0u8; *size];

        let now = Instant::now();

        for _ in 0..iterations {
            rng.fill_bytes(&mut buf);
        }

        let elapsed = now.elapsed();
        let iterations_per_sec = (iterations as f64) / elapsed.as_secs_f64();
        println!("Request size: {size} | Elapsed: {elapsed:.2?} | Rate: {:.2?} MB/s | Iterations: {iterations_per_sec:.2?} 1/s", (iterations * buf.len()) as f64 / elapsed.as_secs_f64() / 1000.0 / 1000.0);
    }
}

/*
 * ESDM
 */
struct BenchmarkEsdm {
    rng: EsdmRng,
}

impl Default for BenchmarkEsdm {
    fn default() -> Self {
        BenchmarkEsdm {
            rng: EsdmRng::new(EsdmRngType::FullySeeded),
        }
    }
}

impl Benchmark for BenchmarkEsdm {
    fn fill_bytes(&mut self, buf: &mut [u8]) {
        self.rng.fill_bytes(buf);
    }
}

/*
 * OsRng
 */
struct BenchmarkOsRng {}

impl Default for BenchmarkOsRng {
    fn default() -> Self {
        BenchmarkOsRng {}
    }
}

impl Benchmark for BenchmarkOsRng {
    fn fill_bytes(&mut self, buf: &mut [u8]) {
        OsRng::default().try_fill_bytes(buf).unwrap();
    }
}

/*
 * ChaCha20
 */
struct BenchmarkChaCha20 {
    rng: ChaCha20Rng,
}

impl Default for BenchmarkChaCha20 {
    fn default() -> Self {
        BenchmarkChaCha20 {
            rng: ChaCha20Rng::from_os_rng(),
        }
    }
}

impl Benchmark for BenchmarkChaCha20 {
    fn fill_bytes(&mut self, buf: &mut [u8]) {
        self.rng.fill_bytes(buf);
    }
}

fn main() {
    println!("ESDM:");
    let mut rng_esdm = BenchmarkEsdm::default();
    benchmark_rng(&mut rng_esdm);

    println!();

    println!("getrandom/OsRng:");
    let mut rng_os = BenchmarkOsRng::default();
    benchmark_rng(&mut rng_os);

    println!();

    println!("rand_chacha (ChaCha20):");
    let mut rng_chacha = BenchmarkChaCha20::default();
    benchmark_rng(&mut rng_chacha);
}

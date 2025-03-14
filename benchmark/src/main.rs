fn main() {
    use std::time::Instant;
    use rand_esdm::{EsdmRng, EsdmRngType};
    use rand::RngCore;

    let iterations = 20000;

    let now = Instant::now();
    let mut rng = EsdmRng::new(EsdmRngType::FullySeeded);

    let sizes = vec![32, 64, 512, 1024, 2048, 4096];
    for size in sizes {
        let mut buf = vec![0u8; size];
        for _ in 0..iterations {
            rng.fill_bytes(&mut buf);
        }
    
        let elapsed = now.elapsed();
        println!("Request size: {size}");
        println!("Elapsed: {:.2?}", elapsed);
        println!("Rate: {:.2?} MB/s", (iterations * buf.len()) as f64 / elapsed.as_secs_f64() / 1000.0 / 1000.0);
    }
}
//! Isolated native-encoder retention check, separate from platform-independent
//! packet ownership tests. Run each concurrency in a fresh test process.
use super::*;

fn peak_rss_bytes() -> u64 {
    let mut usage = std::mem::MaybeUninit::<libc::rusage>::uninit();
    assert_eq!(
        unsafe { libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr()) },
        0
    );
    let peak = unsafe { usage.assume_init() }.ru_maxrss as u64;
    if cfg!(target_os = "macos") {
        peak
    } else {
        peak * 1024
    }
}

#[test]
#[ignore = "isolated native conversion memory benchmark; run with --exact --ignored --nocapture"]
fn repeated_conversion_memory_benchmark() {
    let workers: usize = std::env::var("HIFIMULE_LOCAL_MEMORY_WORKERS")
        .unwrap_or_else(|_| "8".into())
        .parse()
        .unwrap();
    assert!(
        [8, 16].contains(&workers),
        "benchmark supports 8 or 16 native jobs"
    );
    let dir = tempfile::tempdir().unwrap();
    let wav = dir.path().join("source.wav");
    let mp3 = dir.path().join("source.mp3");
    tests::wav(&wav, 10);
    let profile = TranscodeProfile {
        container: Some("mp3".into()),
        audio_codec: Some("mp3".into()),
        max_bitrate_kbps: Some(192),
        ..Default::default()
    };
    encode(
        &wav,
        &mp3,
        &profile,
        &AtomicBool::new(false),
        4 * 1024 * 1024,
    )
    .unwrap();
    let source_bytes_per_pair =
        std::fs::metadata(&wav).unwrap().len() + std::fs::metadata(&mp3).unwrap().len();
    let run_wave = || {
        std::thread::scope(|scope| {
            let handles: Vec<_> = (0..workers)
                .map(|worker| {
                    let input = if worker % 2 == 0 { &wav } else { &mp3 };
                    let output = dir.path().join(format!("output-{worker}.mp3"));
                    let profile = &profile;
                    scope.spawn(move || {
                        let result = encode(
                            input,
                            &output,
                            profile,
                            &AtomicBool::new(false),
                            4 * 1024 * 1024,
                        );
                        result.unwrap_or_else(|error| panic!("conversion failed: {error:#}"));
                        std::fs::remove_file(output).unwrap();
                    })
                })
                .collect();
            for handle in handles {
                handle.join().unwrap();
            }
        });
    };
    // Initialize both codecs at the requested concurrency before measuring.
    run_wave();
    run_wave();
    let baseline = peak_rss_bytes();
    let start = std::time::Instant::now();
    const JOBS: usize = 256;
    let mut samples = vec![baseline];
    for _ in 0..JOBS / workers {
        run_wave();
        let peak = peak_rss_bytes();
        samples.push(peak);
        assert!(
            peak.saturating_sub(baseline) < 512 * 1024 * 1024,
            "memory benchmark exceeded its 512 MiB growth stop bound"
        );
    }
    let growth = peak_rss_bytes().saturating_sub(baseline);
    println!(
        "LOCAL_ENCODING_MEMORY {}",
        serde_json::json!({
            "workers": workers,
            "measured_conversions": JOBS,
            "warmup_conversions": workers * 2,
            "audio_seconds": JOBS * 10,
            "source_bytes": source_bytes_per_pair * (JOBS / 2) as u64,
            "elapsed_seconds": start.elapsed().as_secs_f64(),
            "baseline_peak_rss_bytes": baseline,
            "final_peak_rss_bytes": peak_rss_bytes(),
            "peak_growth_bytes": growth,
            "peak_rss_samples_bytes": samples,
        })
    );
    // Supplementary check: allocator high-water retention can vary. The normal
    // packet-reference regression is the deterministic correctness gate.
    assert!(
        growth < 64 * 1024 * 1024,
        "native memory grew by {growth} bytes after warm-up"
    );
}

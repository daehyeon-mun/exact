//! The Apple host's side of the commit-scale gate: the series of the Linux
//! host's example (`host/linux/examples/commit_scale/fixture.rs`), each
//! booted on this host's Rust with no presenter, and the time one input's
//! `dispatch_at` takes as the app grows: the runner's dispatch, the kernel's
//! layout and this host's emitters (frames, paint order, collections, the
//! batch's text). No Swift runs, so what AppKit or UIKit does with the batch
//! is not in it.
//!
//!   cargo run --profile host-dev -p exact-apple --example commit_scale_apple
//!
//! `bun scripts/metrics.mjs --commit-scale` runs it. One JSON line on stdout.

#[path = "../../linux/examples/commit_scale/fixture.rs"]
mod fixture;

use exact_apple::Host;
use fixture::{summary, time, Measurer, Series, SAMPLES, SIZES, WARMUP};

/// One series at one size: a row of the output.
fn row(series: Series, nodes: usize) -> String {
    const SIZE: (f32, f32) = (1280.0, 800.0);
    let plan = contract::compile(&fixture::source(series, nodes)).expect("the fixture compiles");
    let measurer = Box::new(Measurer::default());
    let (mut host, _first) =
        Host::boot(&plan.encode(), (), measurer, SIZE.0, SIZE.1).expect("the host boots");
    let kernel = host.runner().kernel();
    let live = kernel.live_count();
    let key = kernel.find_by_test_id(series.target())[0];
    let view = kernel.node_by_key(key).expect("a live node").id;
    let mut host_ms = Vec::with_capacity(SAMPLES);
    for n in 0..WARMUP + SAMPLES {
        let at = 16.0 * (n + 1) as f64;
        let (batch, ms) = time(|| host.dispatch_at(view, series.event(n), at));
        assert!(batch.ends_with("\"error\":null}"), "{batch}");
        // The mount series' first input and every second one after it makes
        // views; no other input of any series does.
        let made = batch.matches("{\"op\":\"create\"").count();
        assert_eq!(made, series.mounted() * (1 - n % 2), "views made");
        if n >= WARMUP {
            host_ms.push(ms);
        }
    }
    format!(
        "{{\"host\":\"apple\",\"series\":\"{}\",\"nodes\":{nodes},\"live_nodes\":{live},\"samples\":{SAMPLES},\"host_commit_ms\":{}}}",
        series.name(),
        summary(host_ms),
    )
}

fn main() {
    // The first app a process times pays its cold caches: one goes untimed.
    row(Series::Static, SIZES[0]);
    let mut rows = Vec::new();
    for series in Series::ALL {
        for nodes in SIZES {
            rows.push(row(series, nodes));
        }
    }
    println!(
        "{{\"commit_scale\":[{}],\"commit_scale_note\":\"host-dev; host_commit is the Apple host's dispatch_at in Rust alone (runner dispatch, kernel layout, this host's emitters and the batch's text; no presenter); the monospace measurer, answering for native controls; {}\"}}",
        rows.join(","),
        fixture::note()
    );
}

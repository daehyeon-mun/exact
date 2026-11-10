//! The framework's commit-scale numbers: a generated app of N nodes and one
//! input that changes one node, booted on this host with no display, and the
//! time that input's commit takes as N grows. The work an input asks for is the same
//! at every N, so a pass over the whole tree, over the whole app's state, or
//! over every native control or list, shows as a slope. The series are
//! `commit_scale/fixture.rs`'s.
//!
//!   cargo run --profile host-dev -p exact-linux --example commit_scale
//!
//! `bun scripts/metrics.mjs --commit-scale` runs it beside the Apple host's
//! (`host/apple/examples/commit_scale_apple.rs`) and holds each curve to its
//! budget (`scripts/commit-scale.mjs`). One JSON line on stdout.

#[path = "commit_scale/fixture.rs"]
mod fixture;

use exact_kernel::{Kernel, Offer};
use exact_linux::Host;
use exact_runner::Runner;
use fixture::{summary, time, Measurer, Series, SAMPLES, SIZES, WARMUP};

fn view_of(kernel: &Kernel, test_id: &str) -> u32 {
    let key = kernel.find_by_test_id(test_id)[0];
    kernel.node_by_key(key).expect("a live node").id
}

/// One series at one size: a row of the output.
fn row(series: Series, nodes: usize) -> String {
    const SIZE: (f32, f32) = (1280.0, 800.0);
    let plan = contract::compile(&fixture::source(series, nodes)).expect("the fixture compiles");
    let plan_nodes = plan.nodes.len();
    let (slots, derives) = (plan.slots.len(), plan.derives.len());

    // The host's commit: the runner's dispatch, then what this host does
    // with the receipt up to and including layout.
    let measurer = Box::new(Measurer::default());
    let (mut host, error) =
        Host::boot(&plan.encode(), (), measurer, SIZE.0, SIZE.1).expect("the host boots");
    assert!(error.is_none(), "{error:?}");
    let live = host.kernel().live_count();
    let view = view_of(host.kernel(), series.target());
    let mut host_ms = Vec::with_capacity(SAMPLES);
    for n in 0..WARMUP + SAMPLES {
        let at = 16.0 * (n + 1) as f64;
        let (refused, ms) = time(|| host.dispatch_at(view, series.event(n), at));
        assert!(refused.is_none(), "{refused:?}");
        if n >= WARMUP {
            host_ms.push(ms);
        }
    }
    drop(host);

    // The same commit on the runner alone, and the kernel's layout after
    // it, to say which part a slope is in.
    let kernel = Kernel::new(Box::new(Measurer::default()));
    let mut runner =
        Runner::boot(plan, (), kernel, Default::default(), "/").expect("the runner boots");
    let root = runner.roots()[0];
    let offer = Offer::definite(SIZE.0, SIZE.1);
    runner.kernel_mut().compute_layout(root, offer).unwrap();
    let view = view_of(runner.kernel(), series.target());
    let (mut runner_ms, mut layout_ms) = (Vec::new(), Vec::new());
    for n in 0..WARMUP + SAMPLES {
        let (receipt, commit) = time(|| runner.dispatch(view, series.event(n)).unwrap());
        assert_eq!(receipt.touched.len(), 1, "one node changed");
        let made = receipt.created.len() + receipt.destroyed.len();
        assert_eq!(made, series.mounted(), "nodes made or removed");
        let (_, layout) = time(|| runner.kernel_mut().compute_layout(root, offer).unwrap());
        if n >= WARMUP {
            runner_ms.push(commit);
            layout_ms.push(layout);
        }
    }
    format!(
        "{{\"host\":\"linux\",\"series\":\"{}\",\"nodes\":{nodes},\"plan_nodes\":{plan_nodes},\"live_nodes\":{live},\"slots\":{slots},\"derives\":{derives},\"samples\":{SAMPLES},\"host_commit_ms\":{},\"runner_commit_ms\":{},\"kernel_layout_ms\":{}}}",
        series.name(),
        summary(host_ms),
        summary(runner_ms),
        summary(layout_ms),
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
        "{{\"commit_scale\":[{}],\"commit_scale_note\":\"host-dev; host_commit is the Linux host's dispatch_at with no display (runner dispatch, the host's commit and its layout; no paint); runner_commit and kernel_layout are the same input on a bare runner; the monospace measurer, answering for native controls; {}\"}}",
        rows.join(","),
        fixture::note()
    );
}

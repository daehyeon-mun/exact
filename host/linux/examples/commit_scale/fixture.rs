//! The generated apps of the commit-scale gate and how one is timed, shared
//! by the Linux host's example and the Apple host's
//! (`host/apple/examples/commit_scale_apple.rs`): each series is an app of N nodes
//! and one input whose commit asks for the same work at every N.

use exact_kernel::{
    ButtonMeasure, ButtonMeasureRequest, FieldChrome, FieldChromeRequest, MonospaceMeasurer,
    TextMeasureRequest, TextMeasurer, TextMetrics,
};
use exact_runner::Event;
use std::time::Instant;

/// The app sizes measured, in generated nodes.
pub const SIZES: [usize; 3] = [1_000, 5_000, 20_000];
/// Children of one generated column.
const FAN: usize = 10;
/// Inputs discarded before the measured ones.
pub const WARMUP: usize = 8;
/// Inputs measured.
pub const SAMPLES: usize = 60;
/// The stateful series: one unrelated `state` to this many generated nodes,
/// each read by a text of its own.
const NODES_PER_STATE: usize = 4;
/// The stateful series: one unrelated `derive` to this many generated nodes,
/// each over a state of its own and read by a text.
const NODES_PER_DERIVE: usize = 200;
/// The controls series: one generated leaf in this many is a native control.
const LEAVES_PER_CONTROL: usize = 10;
/// The collection series: the rows of its virtualized list.
const LIST_ROWS: usize = 200;
/// The mount series: the texts of the column a press mounts or unmounts.
const MOUNTED: usize = 8;

/// One generated app and the input timed on it.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Series {
    /// Static texts in columns; a press changes one text.
    Static,
    /// The same with a large app's state: states and derives no press touches.
    Stateful,
    /// The same with native controls among the leaves: buttons, switches
    /// and text fields.
    Controls,
    /// Static texts and one virtualized list.
    Collection,
    /// The controls and the list together; the input is a pointer entering
    /// and leaving a row whose background reads a state, which moves no box.
    Hover,
    /// Static texts; a press mounts a small column below them, the next
    /// unmounts it.
    Mount,
}

impl Series {
    pub const ALL: [Series; 6] = [
        Series::Static,
        Series::Stateful,
        Series::Controls,
        Series::Collection,
        Series::Hover,
        Series::Mount,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Series::Static => "static",
            Series::Stateful => "stateful",
            Series::Controls => "controls",
            Series::Collection => "collection",
            Series::Hover => "hover",
            Series::Mount => "mount",
        }
    }

    /// The `testId` the input goes to.
    pub fn target(self) -> &'static str {
        match self {
            Series::Hover => "hover",
            _ => "bump",
        }
    }

    /// The `n`th input: each changes one node and creates none, but the
    /// mount series', which creates or destroys [`Series::mounted`] nodes.
    pub fn event(self, n: usize) -> Event {
        match self {
            Series::Hover => Event::Hover(n.is_multiple_of(2)),
            _ => Event::Press,
        }
    }

    /// The nodes an input of the mount series creates or destroys; none in
    /// any other.
    pub fn mounted(self) -> usize {
        match self {
            Series::Mount => MOUNTED + 1,
            _ => 0,
        }
    }

    fn controls(self) -> bool {
        matches!(self, Series::Controls | Series::Hover)
    }

    fn list(self) -> bool {
        matches!(self, Series::Collection | Series::Hover)
    }
}

/// A subtree of `nodes` generated leaves and columns at `indent`: a leaf, or
/// a column of up to [`FAN`] subtrees. `leaf` writes each leaf at its indent.
fn subtree(
    out: &mut String,
    indent: usize,
    nodes: usize,
    leaf: &mut dyn FnMut(&mut String, usize),
) {
    if nodes == 1 {
        leaf(out, indent);
        return;
    }
    out.push_str(&" ".repeat(indent));
    out.push_str("column\n");
    let (rest, kids) = (nodes - 1, FAN.min(nodes - 1));
    for k in 0..kids {
        let share = rest / kids + usize::from(k < rest % kids);
        subtree(out, indent + 2, share, leaf);
    }
}

/// The generated app: `nodes` leaves and columns under a button whose text
/// the action `bump` changes.
pub fn source(series: Series, nodes: usize) -> String {
    let (states, derives) = match series {
        Series::Stateful => (nodes / NODES_PER_STATE, nodes / NODES_PER_DERIVE),
        _ => (0, 0),
    };
    let mut out = String::new();
    if series.list() {
        out.push_str("shape Item\n  id: number\n\n");
    }
    out.push_str("component App\n  state count = 0\n");
    if series == Series::Hover {
        out.push_str("  state hot = false\n");
    }
    if series.list() {
        out.push_str(&format!(
            "  state items = range({LIST_ROWS}).map(i => Item(id=i))\n"
        ));
    }
    for k in 0..states {
        out.push_str(&format!("  state s{k} = {k}\n"));
    }
    for k in 0..derives {
        out.push_str(&format!("  derive d{k} = s{k} + 1\n"));
    }
    if series == Series::Mount {
        out.push_str("  state open = false\n  action bump\n    open = not open\n");
    } else {
        out.push_str("  action bump\n    count = count + 1\n");
    }
    if series == Series::Hover {
        out.push_str("  action hover(over: bool)\n    hot = over\n");
    }
    out.push_str("  view\n    column\n");
    out.push_str("      button press=bump testId=\"bump\"\n        text `${count}`\n");
    if series == Series::Hover {
        out.push_str(
            "      row hover=hover background-color=(hot ? \"#d4d4d8\" : \"#00000000\") testId=\"hover\"\n        text \"Hover\"\n",
        );
    }
    if series.list() {
        out.push_str(
            "      list virtualized=true height=160 width=240\n        for item in items key=item.id\n          text `${item.id}`\n",
        );
    }
    let mut leaves = 0;
    subtree(&mut out, 6, nodes, &mut |out, indent| {
        let pad = " ".repeat(indent);
        if series.controls() && leaves % LEAVES_PER_CONTROL == LEAVES_PER_CONTROL - 1 {
            // A button, a switch and a text field in turn.
            match (leaves / LEAVES_PER_CONTROL) % 3 {
                0 => out.push_str(&format!("{pad}button press=bump\n{pad}  text \"Change\"\n")),
                1 => out.push_str(&format!(
                    "{pad}input type=\"checkbox\" switch aria-label=\"Setting {leaves}\"\n"
                )),
                _ => out.push_str(&format!("{pad}input placeholder=\"Field {leaves}\"\n")),
            }
        // The first texts read the derives, the next the states, the rest
        // are literals.
        } else if leaves < derives {
            out.push_str(&format!("{pad}text `${{d{leaves}}}`\n"));
        } else if leaves < states {
            out.push_str(&format!("{pad}text `${{s{leaves}}}`\n"));
        } else {
            out.push_str(&format!("{pad}text \"{leaves}\"\n"));
        }
        leaves += 1;
    });
    // Mounted after everything else, so that no other box moves.
    if series == Series::Mount {
        out.push_str("      if open\n        column\n");
        for k in 0..MOUNTED {
            out.push_str(&format!("          text \"mounted {k}\"\n"));
        }
    }
    out
}

/// The monospace measurer, answering for native controls as a host that
/// draws the platform's own does: a button's fitting box and a field's
/// chrome, each final at the first ask.
#[derive(Default)]
pub struct Measurer(MonospaceMeasurer);

impl TextMeasurer for Measurer {
    fn measure(&mut self, request: &TextMeasureRequest<'_>) -> TextMetrics {
        self.0.measure(request)
    }

    fn height_free(&self) -> bool {
        self.0.height_free()
    }

    fn button_measure(&mut self, _: &ButtonMeasureRequest) -> Option<ButtonMeasure> {
        Some(ButtonMeasure {
            width: 80.0,
            height: 24.0,
            provisional: false,
        })
    }

    fn field_chrome(&mut self, _: &FieldChromeRequest) -> FieldChrome {
        FieldChrome {
            top: 3.0,
            right: 5.0,
            bottom: 3.0,
            left: 5.0,
            minimum_height: 22.0,
            ..FieldChrome::default()
        }
    }
}

pub fn summary(mut samples: Vec<f64>) -> String {
    samples.sort_by(f64::total_cmp);
    format!(
        "{{\"min\":{:.4},\"p50\":{:.4},\"p95\":{:.4}}}",
        samples[0],
        samples[samples.len() / 2],
        samples[(samples.len() * 95).div_ceil(100) - 1]
    )
}

pub fn time<T>(f: impl FnOnce() -> T) -> (T, f64) {
    let started = Instant::now();
    let value = f();
    (value, started.elapsed().as_secs_f64() * 1e3)
}

/// What every row's note says of the fixture.
pub fn note() -> String {
    format!(
        "{WARMUP} warmup inputs, {SAMPLES} measured, each changing one node (the mount series': making or removing a few); static: N texts in columns {FAN} wide, a press changes one text; stateful: the same with one unrelated state per {NODES_PER_STATE} nodes and one unrelated derive per {NODES_PER_DERIVE}, each read by a text; controls: one leaf in {LEAVES_PER_CONTROL} a native button, switch or text field; collection: the static app with a virtualized list of {LIST_ROWS} rows; hover: the controls and the list, and the input a pointer entering or leaving a row whose background reads a state; mount: the static app, a press mounting a column of {MOUNTED} texts below it and the next unmounting it"
    )
}

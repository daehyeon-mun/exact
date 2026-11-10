//! A walk looking for the mounted lists enters only the plan sites a list
//! can be under (`instance/collection/traversal.rs`, `ListSites`): wherever
//! the plan puts a list, the walk still finds it, beside any number of boxes
//! that hold none.
use exact_kernel::Kernel;
use exact_runner::Runner;

/// Lists in an `if` arm, in a keyed row, in a component and in another
/// list's row, and one that opts out, among `filler` boxes with no list
/// under them.
fn source(filler: usize) -> String {
    let mut out = String::from(
        r#"shape Item
  id: number

component App
  state items = range(6).map(i => Item(id=i))
  state shown = true
  action hide
    shown = false
  view
    column
      button press=hide testId="hide"
        text "Hide"
      if shown
        list virtualized=true height=80 width=200 testId="in-arm"
          for item in items key=item.id
            text `${item.id}`
      for group in items.slice(0, 2) key=group.id
        row
          list virtualized=true height=80 width=200 testId=`in-row-${group.id}`
            for item in items key=item.id
              text `${item.id}`
      Inner(items=items)
      list virtualized=true height=120 width=200 testId="outer"
        for outer in items.slice(0, 2) key=outer.id
          list virtualized=true height=40 width=200 testId=`nested-${outer.id}`
            for item in items key=item.id
              text `${item.id}`
      list virtualized=false height=80 width=200 testId="eager"
        for item in items key=item.id
          text `${item.id}`
"#,
    );
    for i in 0..filler {
        out.push_str(&format!(
            "      column\n        text \"filler {i}\"\n        text \"more {i}\"\n"
        ));
    }
    out.push_str(
        r#"
component Inner
  props
    items: Item[]
  view
    column
      list virtualized=true height=80 width=200 testId="in-component"
        for item in items key=item.id
          text `${item.id}`
"#,
    );
    out
}

fn boot(filler: usize) -> Runner<()> {
    let plan = contract::compile(&source(filler)).expect("the fixture compiles");
    let kernel = Kernel::with_monospace();
    Runner::boot(plan, (), kernel, Default::default(), "/").expect("the runner boots")
}

fn lists(runner: &Runner<()>) -> Vec<String> {
    let kernel = runner.kernel();
    let mut names: Vec<String> = runner
        .collections()
        .iter()
        .map(|c| {
            let node = kernel.node(c.view).expect("a live list");
            node.props
                .str(exact_kernel::PropId::TestId)
                .unwrap_or("?")
                .to_string()
        })
        .collect();
    names.sort();
    names
}

#[test]
fn lists_are_found_wherever_the_plan_puts_them_among_boxes_that_hold_none() {
    for filler in [0, 500] {
        let mut runner = boot(filler);
        let all = [
            "in-arm",
            "in-component",
            "in-row-0",
            "in-row-1",
            "nested-0",
            "nested-1",
            "outer",
        ];
        assert_eq!(lists(&runner), all, "{filler} boxes beside them");
        let shallow: Vec<_> = runner.collections_shallow();
        assert_eq!(shallow.len(), all.len());
        assert_eq!(runner.collection_data().len(), all.len());
        for snapshot in &shallow {
            assert!(runner.collection(snapshot.view).is_some());
        }
        // The arm's list goes with its arm.
        let kernel = runner.kernel();
        let hide = kernel.find_by_test_id("hide")[0];
        let hide = kernel.node_by_key(hide).unwrap().id;
        runner.dispatch(hide, exact_runner::Event::Press).unwrap();
        assert_eq!(lists(&runner), all[1..]);
    }
}

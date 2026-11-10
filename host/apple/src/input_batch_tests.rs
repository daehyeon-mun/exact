//! Cancellation affects native defaults; propagation controls listener delivery.
use super::*;
use exact_kernel::MonospaceMeasurer;
use exact_plan::Value;
use exact_runner::Event;

fn boot(command: &str) -> Host<()> {
    let source = format!(
        r#"component App
  state heard = ""
  action first
    heard = `${{heard}}a`
    {command}()
  action second
    heard = `${{heard}}b`
  action input
    heard = `${{heard}}i`
  view
    column
      button press=first testId="first"
        text "First"
      button press=second testId="second"
        text "Second"
      button press=input testId="input"
        text "Input"
"#
    );
    let plan = contract::compile(&source).unwrap().encode();
    Host::boot(
        &plan,
        (),
        Box::new(MonospaceMeasurer::default()),
        400.0,
        400.0,
    )
    .unwrap()
    .0
}

fn id(host: &Host<()>, name: &str) -> ViewId {
    let kernel = host.runner.kernel();
    kernel
        .node_by_key(kernel.find_by_test_id(name)[0])
        .unwrap()
        .id
}

#[test]
fn prevented_echo_keeps_key_listeners_but_skips_native_input() {
    let mut host = boot("preventDefault");
    let entries = vec![
        (id(&host, "first"), Event::Press, 3),
        (id(&host, "second"), Event::Press, 3),
        (id(&host, "input"), Event::Press, 2),
    ];
    let batch = host.dispatch_many(entries, 0.0);
    assert!(serde_json::from_str::<serde_json::Value>(&batch).unwrap()["error"].is_null());
    assert_eq!(host.runner.slot("heard"), Some(&Value::str("ab")));
}

#[test]
fn stopped_path_keeps_the_native_input_default() {
    let mut host = boot("stopPropagation");
    let entries = vec![
        (id(&host, "first"), Event::Press, 3),
        (id(&host, "second"), Event::Press, 3),
        (id(&host, "input"), Event::Press, 2),
    ];
    host.dispatch_many(entries, 0.0);
    assert_eq!(host.runner.slot("heard"), Some(&Value::str("ai")));
}

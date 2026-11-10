# Recipe: fast interactions

Time an interaction from its input to the pixels it changes, the host's work and the
display's refresh included.

## Keep local feedback in the input's action

Change what the input shows in the input's own commit (a disclosure, a draft):

```contract
component Disclosure
  state expanded = false
  action toggle
    expanded = not expanded
  view
    column testId="disclosure"
      button press=toggle aria-expanded=expanded testId="toggle"
        text "Details"
      if expanded
        text "Details." testId="details"
```

```contract-test
test "a disclosure opens in the input's commit"
  tap "toggle"
  expect tree has "details"
  tap "toggle"
  expect tree missing "details"
```

What the server decides still waits for it: show that a write is pending, keep the
draft and any refusal, and close a save form only on success
([after-a-write](after-a-write.md)). Leave work that reads geometry to layout, and an
embedded view's handshake to its load.

## Bound what an input invalidates and what stays alive

- Key a resource by the data it reads; gate a hidden page's work, refresh when shown.
- Keep a row's own hover and draft in the keyed row, shared state with its owner.
- Pass summaries; fetch the selected item's details on demand, invalidated explicitly.
- Run a [timer](timers.md) only while it has work; keep its background deadlines.
- Keep typing local; send on submit or debounced, keeping cancellation.
- A detached subtree loses its authored children, controls and subscriptions, and a
  queued observer of a removed target is dropped: test focus, accessibility, a reset
  and two sessions.
- Coalesce paint, but never delay a committed input gate or accessibility state.
- Cache immutable work keyed by everything it reads, with an owner that ends it; test
  another window, a restart and a first launch with a cold disk cache.

## Keep what is drawn

On macOS `backdrop-filter` uses Core Image, and `backgroundMaterial` draws other
pixels. Compare light and dark, and text behind the surface. Removing authored blur or
motion changes the result, so it does not count as an optimization.

## Measure before adopting a rule

An isolated probe can miss a pixel change or a cost the whole screen adds, so measure
composed screens too. Count the work an input does:

```sh
bun exact.mjs agent macos --storage perf-check --json \
  'perf disclosure during "tap toggle"' state logs
```

Measure a first use and repeats, a refused input, rapid input and cancellation.
Compare release builds in one quiet session, alternating them, 5 to 10 launches each,
chosen beforehand; start every launch from the same state, time one-time setup
separately, and keep the builds and raw timings. Round intervals to the timestamps'
precision before ranking ties. Accept a change when the difference's bootstrap 95%
confidence interval lies wholly on the better side, a two-sided Mann–Whitney test gives
p < 0.05, nothing else regresses, and the app looks and behaves as before. An interval
that crosses zero is unresolved, not a tie.

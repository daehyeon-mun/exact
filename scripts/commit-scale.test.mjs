import { describe, expect, test } from 'bun:test';
import { COMMIT_SCALE_LIMIT, COMMIT_SCALE_RUNS, commitScaleEmit, commitScaleVerdicts } from './commit-scale.mjs';

const row = (series, nodes, p50, host) => ({ ...(host && { host }), series, nodes, host_commit_ms: { p50 } });

describe('the one-op commit as the app grows', () => {
  test('each series is held to the ratio of its largest app to its smallest, whatever the order', () => {
    const rows = [row('static', 20000, 0.12), row('static', 1000, 0.1), row('static', 5000, 0.4),
      row('stateful', 1000, 0.1), row('stateful', 20000, 0.8)];
    expect(COMMIT_SCALE_LIMIT).toBe(1.5);
    const [flat, sloped] = commitScaleVerdicts(rows);
    expect(flat).toMatchObject({ host: 'linux', series: 'static', smallest: 1000, largest: 20000, within: true });
    expect(flat.ratio).toBeCloseTo(1.2);
    expect(sloped).toMatchObject({ series: 'stateful', within: false });
    expect(sloped.ratio).toBeCloseTo(8);
    expect(commitScaleVerdicts(rows, 8)[1].within).toBe(true);
  });

  test('a series is judged on each host by itself', () => {
    const rows = [row('controls', 1000, 0.01, 'linux'), row('controls', 20000, 0.012, 'linux'),
      row('controls', 1000, 0.09, 'apple'), row('controls', 20000, 1.7, 'apple')];
    const verdicts = commitScaleVerdicts(rows);
    expect(verdicts.map(v => [v.host, v.series, v.within])).toEqual([['linux', 'controls', true], ['apple', 'controls', false]]);
    expect(COMMIT_SCALE_RUNS.map(r => r.host)).toEqual(['linux', 'apple']);
  });

  test("a host's emitters are its commit less the runner's and the kernel's parts at that size", () => {
    const bare = { ...row('hover', 20000, 0.004, 'linux'), runner_commit_ms: { p50: 0.002 }, kernel_layout_ms: { p50: 0.0005 } };
    const apple = row('hover', 20000, 2.4, 'apple');
    expect(commitScaleEmit(apple, [bare, apple])).toBeCloseTo(2.3975);
    expect(commitScaleEmit(bare, [bare, apple])).toBeCloseTo(0.0015);
    expect(commitScaleEmit(row('hover', 5000, 1, 'apple'), [bare, apple])).toBeNull();
  });
});

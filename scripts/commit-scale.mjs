/**
 * The budget and verdict of `bun scripts/metrics.mjs --commit-scale`: an input that changes one node
 * asks for the same commit at every app size, so each host's largest app in each series is held to
 * `limit` times its smallest's p50.
 */

/** Flat within a tolerance: the commit at 20,000 nodes costs at most this many times the commit at
 *  1,000. Wide enough for a tree two levels deeper and for timer noise at tens of microseconds; a pass
 *  over the tree, over the app's state or over its native controls is a multiple of it. It is stated
 *  here because `rules/RULES.md`, where the other time budgets are, is at its word cap. */
export const COMMIT_SCALE_LIMIT = 1.5;

/** The examples `--commit-scale` runs, each printing rows for one host. */
export const COMMIT_SCALE_RUNS = [
  { host: 'linux', package: 'exact-linux', example: 'commit_scale' },
  { host: 'apple', package: 'exact-apple', example: 'commit_scale_apple' },
];

const hostOf = row => row.host ?? 'linux';

export function commitScaleVerdicts(rows, limit = COMMIT_SCALE_LIMIT) {
  return [...new Set(rows.map(r => `${hostOf(r)} ${r.series}`))].map(key => {
    const own = rows.filter(r => `${hostOf(r)} ${r.series}` === key).sort((a, b) => a.nodes - b.nodes);
    const first = own[0], last = own[own.length - 1], ratio = last.host_commit_ms.p50 / first.host_commit_ms.p50;
    return { host: hostOf(first), series: first.series, smallest: first.nodes, largest: last.nodes, ratio, within: ratio <= limit };
  });
}

/** What a host's commit spent outside the runner's commit and the kernel's layout, which the Linux
 *  host's example times on a bare runner for the same series and size: the host's own emitters and
 *  bookkeeping. `null` where either part was not measured. */
export function commitScaleEmit(row, rows) {
  const bare = rows.find(r => r.series === row.series && r.nodes === row.nodes && r.runner_commit_ms);
  if (!bare) return null;
  return Math.max(0, row.host_commit_ms.p50 - bare.runner_commit_ms.p50 - bare.kernel_layout_ms.p50);
}

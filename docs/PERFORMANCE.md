# Performance

No formal performance targets have been published and no benchmark harness is claimed to exist. Define numeric budgets from measured workloads on supported browsers and hosts.

## Design constraints

- Simulation advances on a fixed timestep (60 Hz) independent of render frame rate.
- Rendering, networking, and simulation costs should be measured separately where possible.
- Server capacity and browser frame-time budgets are open until representative workloads exist. Generated tracks involve many noise evaluations, so `racer-trackgen` is built with optimizations even in debug test runs.

## Reporting

For each benchmark, record commit, toolchain and build mode, hardware and OS, workload size, warmup and sample method, latency/throughput/frame-time distribution, and limitations. Avoid claims based on a single run or a different workload.

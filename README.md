
# REDLINE

REDLINE is a powerful benchmarking tool designed for load testing MagicBlock validators. It enables developers and operators to simulate high-load scenarios and observe the performance of their validator nodes. With its flexible configuration options and multiple modes of operation, REDLINE is an essential tool for anyone looking to optimize the performance of their Solana infrastructure.

## Features

- **Multi-threaded Execution**: REDLINE is capable of running multiple benchmarks concurrently, using different keypairs, which are used to derive many different PDA accounts to be used in benchmark transactions, this simulates real-world load on the validator.
  
- **Configurable Transaction Per Second (TPS)**: Users can specify the desired TPS to target during the benchmark, allowing them to simulate different load levels and understand how the validator performs under pressure.

- **Customizable Connection Settings**: Supports both HTTP1 and HTTP2, with configurable maximum connections for both HTTP and WebSocket protocols. REDLINE has a very tight low level control over network IO to ensure accurate measurements.

- **Flexible Benchmark Modes**: REDLINE supports a range of benchmark modes to target specific performance areas:
  - **Simple Byte Set**: Tests basic transaction throughput.
  - **Trigger Clones**: Simulates the overhead of handling multiple read-only accounts, which are regularly updated on main chain, thus forcing them to be recloned.
  - **High Compute Cost**: Stresses the validator with transactions that require significant compute resources.
  - **Read and Write Across Accounts**: Evaluates the performance of simultaneous read and write operations, with multiple transactions using intersecting set of accounts, thus creating lock contention.
  - **Mixed Mode**: Combines multiple transaction types to simulate complex workloads.

- **Detailed Latency Tracking**: REDLINE provides granular insights into transaction and event confirmation latencies, helping to identify bottlenecks.

- **Comprehensive Statistics**: After each benchmark, REDLINE generates detailed statistics including latency distributions, TPS achieved, and more.

## Getting Started

To start using REDLINE, clone the repository to your local environment. Configuration is managed through a TOML file, which allows you to specify connection settings, benchmarking parameters, and more.

Once configured, simply run the REDLINE executable with your configuration file to begin the benchmark. REDLINE will handle the rest, providing detailed outputs and statistics upon completion.

## Usage Example

```bash
redline path/to/config.toml
```

## Configuration

REDLINE uses a TOML configuration file to manage its settings. Here is an example configuration:
```toml
[connection]
chain-url = "http://api.devnet.solana.com"
ephem-url = "http://127.0.0.1:8899"
http-connection-type = "http1"

[benchmark]
iterations = 15000
tps = 100
concurrency = 16
preflight-check = true
keypairs = ["keypairs/1.json"]
mode = "simple-byte-set"

[subscription]
subscribe-to-accounts = true
subscribe-to-signatures = true
enforce-total-sync = false

[data]
account-encoding = "base64+zstd"
account-size = "bytes128"
```
See `config.example.toml` for detail explanation of various options

## Conclusion

REDLINE is a robust, versatile tool for anyone involved in the MagicBlock ecosystem looking to test and improve their validator performance. Its range of features and modes of operation make it suitable for a wide variety of testing scenarios, providing various insights into validator performance profile.

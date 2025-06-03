
# REDLINE

REDLINE is a powerful benchmarking tool designed for load testing MagicBlock validators. It enables developers and operators to simulate high-load scenarios and observe the performance of their validator nodes. With its flexible configuration options and multiple modes of operation, REDLINE is an essential tool for anyone looking to optimize the performance of their Solana infrastructure.

## Features

- **Multi-threaded Execution**: REDLINE is capable of running multiple benchmarks concurrently, using different keypairs, which are used to derive many different PDA accounts to be used in benchmark transactions, this simulates real-world load on the validator.
  
- **Configurable Transaction Per Second (TPS)**: Users can specify the desired TPS to target during the benchmark, allowing them to simulate different load levels and understand how the validator performs under pressure.

- **Configurable Requests Per Second (TPS)**: Users can specify the desired RPS to target during the benchmark, this allows for mixing in read requests like `getAccountInfo` and others to the raw transaction benchmarking.

- **Customizable Connection Settings**: Supports both HTTP1 and HTTP2, with configurable maximum connections for both HTTP and WebSocket protocols. REDLINE has a very tight low level control over network IO to ensure accurate measurements.

- **Flexible Benchmark Modes**: REDLINE supports a range of benchmark modes to target specific performance areas:
  - **Simple Byte Set**: Tests basic transaction throughput.
  - **Trigger Clones**: Simulates the overhead of handling multiple read-only accounts, which are regularly updated on main chain, thus forcing them to be recloned.
  - **High Compute Cost**: Stresses the validator with transactions that require significant compute resources.
  - **Read and Write Across Accounts**: Evaluates the performance of simultaneous read and write operations, with multiple transactions using intersecting set of accounts, thus creating lock contention.
  - **Mixed Mode**: Combines multiple transaction types to simulate complex workloads.
  - **getX** requests: Various account related JSON-RPC requests can be used in addition (or as a standalone benchmark) to the transaction load testing.

- **Detailed Latency Tracking**: REDLINE provides granular insights into transaction and event confirmation latencies, helping to identify bottlenecks.

- **Comprehensive Statistics**: After each benchmark, REDLINE generates detailed statistics including latency distributions, TPS achieved, and more.

## Getting Started

To start using REDLINE, clone the repository to your local environment. Configuration is managed through a TOML file, which allows you to specify connection settings, benchmarking parameters, and more.

Once configured, simply run the REDLINE executable with your configuration file to begin the benchmark. REDLINE will handle the rest, providing detailed outputs and statistics upon completion.

## Usage
REDLINE has two binaries:
1. Main `redline` command to run benchmarks and generate statistics
2. Utility `redline-assist` command to perform some utility functions before or after benchmark

Although you can use them directly, it's more convenient to employ a helper make file to orchestrate the interaction with those binaries.

### Build the binaries
```bash
make build
```
this will build both binaries in release mode

### Prepare the benchmark
It's recommended (especially after changing benchmark modes) to run the
preparation script before benchmark, to ensure that all the necessary solana
accounts are created and delegated 
```bash
make prepare <CONFIG=path-to-config>
```
this will check that all the accounts to be used in the benchmark are in proper state. `CONFIG` environment variable is optional, `config.toml` will be used if not provided. 

### Run the benchmark
```bash
make bench <CONFIG=path-to-config>
```
this will run the benchmark with configured parameters. `CONFIG` environment
variable is optional, `config.toml` will be used if not provided. 

### Print the benchmark report
```bash
make report <OUTPUT=path-to-json-output-file>
```
this will print out detailed statistics of the benchmarking. `OUTPUT`
environment variable is optional, if not provided, the last benchmark result
will be printed

### Run the benchmark and print the report
```bash
make bench-report
```

### Compare benchmark results
```bash
make compare <SENSITIVITY=NUM> <THIS=path-to-json-output-file1> <THAT=path-to-json-output-file2>
```
print the comparison table between two bench runs, SENSITIVITY is number between 0 and 100 (default is 15), which is used to highlight performance anomalies which exceed this threshold (percentage-wise), THIS and THAT environment variables are optional, and if not provided the last two bench runs will be used.

### Run the benchmark, compare with the previous run, and delete the generated result
```bash
make bench-compare
```
this can be useful to repeatedly run the benchmark after tweaks, and comparing the results with fixed previous bench run

### Cleanup
```bash
make clenaup # removes the last benchmark result
make clean-all # removes results of all previous benchmarks
```


## Configuration

REDLINE uses a TOML configuration file to manage its settings. Here is an example configuration:
```toml
parallelism = 4

[connection]
chain-url = "http://api.devnet.solana.com"
ephem-url = "http://127.0.0.1:8899"
http-connection-type = "http1"

[rps-benchmark]
enabled = true
iterations = 15000
rps = 100
concurrency = 16
mode = "get-account-info"

[tps-benchmark]
enabled = true
iterations = 15000
tps = 100
concurrency = 16
mode = "simple-byte-set"

[subscription]
subscribe-to-accounts = true
subscribe-to-signatures = true
enforce-total-sync = false

[data]
account-encoding = "base64+zstd"
account-size = "bytes128"
```
See [`config.example.toml`](./config.example.toml) for detailed explanation of the various options

## Conclusion

REDLINE is a robust, versatile tool for anyone involved in the MagicBlock ecosystem looking to test and improve their validator performance. Its range of features and modes of operation make it suitable for a wide variety of testing scenarios, providing various insights into validator performance profile.

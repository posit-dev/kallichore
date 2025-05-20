# Kallichore Server Integration Tests

This directory contains integration tests for the Kallichore Server (kcserver). These tests verify that the server can start, create and manage kernels, execute code, and handle various operations correctly.

## Test Files

- `integration_test.rs`: Basic tests for starting kernels, executing code, and stopping kernels
- `advanced_execution_test.rs`: Tests for multiple code executions with different types of outputs
- `api_test.rs`: Tests for the server's API endpoints
- `echo_kernel_test.rs`: Self-contained tests using an echo kernel instead of Python

## Requirements

To run most tests, you need:

1. A working Python installation with `ipykernel` package installed
2. Rust toolchain

You can install the Python requirements using:

```bash
pip install ipykernel
```

For the `echo_kernel_test.rs` tests, no Python installation is required as they use a simulated echo kernel that is self-contained.

## Running the Tests

Run integration tests using:

```bash
cargo test --test integration_test -- --test-threads=1
cargo test --test advanced_execution_test -- --test-threads=1
cargo test --test api_test -- --test-threads=1
cargo test --test echo_kernel_test -- --test-threads=1
```

Or run all tests at once:

```bash
cargo test -- --test-threads=1
```

**Note**: We use `--test-threads=1` to run tests serially because they start separate server instances on random ports.

## What the Tests Cover

1. **Server lifecycle**:

   - Starting and stopping the server
   - Server configuration management

2. **Session management**:

   - Creating new kernel sessions
   - Starting kernels
   - Listing active sessions
   - Getting session details
   - Killing and deleting sessions

3. **Code execution**:

   - Basic code execution and result retrieval
   - Variable manipulation
   - Function definition and calling
   - Error handling
   - Library imports
   - Complex data structures

4. **Interrupt handling**:

   - Interrupting running code execution

5. **Self-contained tests**:
   - Echo kernel tests that don't require external dependencies
   - Testing kernel execution with a simulated echo kernel
   - Testing multiple executions and interruption

## Troubleshooting

If tests fail, check:

1. Python and ipykernel are installed correctly (for Python-based tests)
2. No other Kallichore servers are running on your machine
3. The tests have enough time to start/stop server components (you may need to increase timeouts on slower machines)
4. For echo kernel tests, ensure the EchoKernel implementation is functioning correctly

## Adding New Tests

When adding new tests, follow these patterns:

1. Use the `TestServer` helper struct to manage server lifecycle
2. Isolate tests to keep them independent
3. Add proper cleanup in case of test failures

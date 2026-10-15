# Gemini Workspace for `gpuinfo-rust` (v0.3.0)

You are a Rust Developer working with Google Cloud.
You should follow Rust Best practices.
The recommended language level for Rust is 2024.

This document provides a developer-focused overview of the `gpuinfo-rust` project, tailored for use with Gemini.

## Project Overview

`gpuinfo-rust` is a Model Context Protocol (MCP) server written in Rust that reports detailed GPU information. It talks over standard input/output (stdio) to MCP clients (like Claude Desktop, Claude Code, Gemini CLI or IDE extensions) and also runs as a CLI.

### Key Technologies

*   **Language:** [Rust](https://www.rust-lang.org/) (Edition 2024)
*   **MCP SDK:** [rmcp](https://crates.io/crates/rmcp) (v0.14.0) - Uses macros for tool definition and routing.
*   **NVIDIA:** [nvml-wrapper](https://crates.io/crates/nvml-wrapper) (v0.13) - loads `libnvidia-ml.so` at runtime, so no NVIDIA library is needed to build.
*   **PCI names:** system `pci.ids`, with [pci-ids](https://crates.io/crates/pci-ids) as the built-in fallback.
*   **Async Runtime:** [Tokio](https://tokio.rs/) - GPU queries run on `spawn_blocking`.
*   **Serialization:** [Serde](https://serde.rs/) & [Schemars](https://crates.io/crates/schemars)
*   **Logging:** [Tracing](https://crates.io/crates/tracing) & [Tracing-Subscriber](https://crates.io/crates/tracing-subscriber) (JSON format to stderr)
*   **Containerization:** [Docker](https://www.docker.com/) (Distroless base)

## Architecture

*   **`src/main.rs`**: entry point.
    *   `GpuInfo` struct: implements `ServerHandler` and `tool_router`.
    *   Tools: `list_gpus` and `get_gpu_info` (optional `index`).
    *   `discover`: merges sysfs PCI GPUs with NVML devices, matched by PCI domain/bus/device.
    *   `list_report` / `info_report`: shared by the MCP tools and the CLI `list` / `info [N]` commands.
*   **`src/sysfs.rs`**: vendor-neutral collection from `/sys/bus/pci/devices` (PCI class 0x03 display and 0x12 accelerator): identity, driver, PCIe link, AER, BARs, DRM nodes and connectors (EDID monitor name), Intel GT frequencies, amdgpu files, hwmon sensors.
*   **`src/nvidia.rs`**: NVML and `/proc/driver/nvidia`. `put` omits fields NVML reports as unsupported and shows other errors. `graphics_clocks` calls NVML directly because nvml-wrapper 0.13 returns zeros when a GPU has more than 128 graphics clock steps.
*   **`src/pciids.rs`**: vendor / device / subsystem / class name lookup.
*   **`src/report.rs`**: text report builder and byte formatting.

## Getting Started

### Prerequisites

*   [Rust Toolchain](https://www.rust-lang.org/tools/install)
*   [Docker](https://docs.docker.com/get-docker/) (for container builds)
*   [Google Cloud SDK](https://cloud.google.com/sdk/docs/install) (for deployment)

### Environment Setup

- `source ./set_env.sh`: Sets up GCP project IDs, regions, and `RUST_LOG=trace`.
- `./set_adc.sh`: Authenticates with Google Application Default Credentials.

### Build and Run

```bash
cargo build
cargo run -- list        # list GPUs
cargo run -- info        # full report, all GPUs
cargo run -- info 0      # full report, GPU 0
make run                 # MCP server on stdio
```

## Development Workflow

*   **Formatting:** `make fmt`
*   **Linting:** `make clippy`
*   **Checking:** `make check`
*   **Testing:** `make test` - covers schema generation, pci.ids parsing, PCI address parsing, EDID decoding, byte formatting, and a full report run on the local machine.

## Deployment

Deployment configuration is managed via `cloudbuild.yaml`; `make deploy` submits it. The `Dockerfile` is a multi-stage build (Rust builder, `gcr.io/distroless/cc-debian12` runtime). NVIDIA detail inside a container requires the NVIDIA Container Toolkit.

## Interacting with Gemini

Relevant examples:

*   "Add an `--json` output mode to `info_report`."
*   "Add Intel Level Zero (`libze_loader`) support alongside NVML in a new module."
*   "Add AMD SMI (`libamd_smi.so`) support for ROCm hosts."
*   "Add a `watch` tool that samples utilization and power over N seconds."

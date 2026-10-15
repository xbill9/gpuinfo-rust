# gpuinfo-rust (v0.3.0)

A Rust [Model Context Protocol (MCP)](https://modelcontextprotocol.io/) server that reports detailed GPU information. It runs over standard input/output (stdio) and also works as a plain command-line tool.

## Overview

`gpuinfo-rust` gathers everything it can find about every GPU on a Linux host from three sources:

*   **sysfs** (`/sys/bus/pci`, `/sys/class/drm`): vendor-neutral, works for NVIDIA, AMD and Intel with no vendor library.
*   **NVML** (`libnvidia-ml.so`): loaded at runtime, so the binary builds and runs on hosts without the NVIDIA driver; NVIDIA detail appears when the library is present.
*   **`/proc/driver/nvidia`**: the NVIDIA kernel module's own version and per-GPU information files.

Device names come from the system `pci.ids` file when installed, with a built-in database as the fallback.

## Tools

| Tool | Arguments | Returns |
| --- | --- | --- |
| `list_gpus` | none | One line per GPU: PCI address, vendor, model, PCI IDs, driver, NVML name and memory size. |
| `get_gpu_info` | `index` (optional) | The full report for GPU `index` (numbers from `list_gpus`), or for every GPU when omitted. |

## What the report contains

**Host:** host name, kernel, GPU count, GPU device nodes (`/dev/dri/*`, `/dev/nvidia*`, `/dev/kfd`, `/dev/accel/*`), NVIDIA driver, NVML and CUDA driver API versions, kernel module build line, and GPU-to-GPU topology when there is more than one NVIDIA GPU.

**Per GPU, any vendor (sysfs):**

*   **PCI:** address, vendor, device, subsystem vendor and device (names and IDs), class, revision.
*   **Driver:** kernel driver, module version and srcversion, modalias.
*   **PCIe link:** current and maximum speed and width, AER correctable / non-fatal / fatal error totals.
*   **Platform:** NUMA node, local CPUs, IOMMU group, IRQ, boot VGA, PCI power state, runtime PM status and control, D3cold, reset methods.
*   **BARs:** address, size and type of each BAR and the expansion ROM, including 64-bit, prefetchable and resizable flags.
*   **DRM:** card and render nodes; each display connector's status, preferred mode and monitor name (decoded from EDID); Intel GT frequencies.
*   **amdgpu:** product name and number, serial, unique ID, VBIOS, DPM state and performance level, GPU and memory busy %, VRAM / visible VRAM / GTT totals and usage, VRAM vendor, shader / memory / fabric / SoC / PCIe DPM levels.
*   **hwmon sensors:** temperatures with labels and critical limits, power average / input / cap, voltages, currents, fan RPM, frequencies, energy.

**Per GPU, NVIDIA (NVML):**

*   **Identity:** product name, brand, architecture, CUDA compute capability, CUDA cores, UUID, serial, board part number and ID, minor number, VBIOS, InfoROM, GSP firmware version and mode, bus type, IRQ, NUMA node, virtualization mode, MIG mode.
*   **Memory:** total / used / free / reserved VRAM, memory bus width, BAR1 total / used / free, ECC mode, volatile and aggregate ECC error counts, retired pages.
*   **Utilization:** GPU and memory controller %, encoder and decoder %, encoder sessions, H.264 / HEVC encoder capacity, FBC sessions.
*   **Clocks:** current, maximum, customer boost and application clocks for graphics, SM, memory and video; auto boost; every supported memory clock with the graphics clock range available at each.
*   **Performance:** P-state, active throttle reasons, compute mode, persistence mode, display connected / active, accounting mode.
*   **Power:** draw, limit, default and enforced limits, allowed range, energy since driver load, power source.
*   **Thermal:** GPU temperature, shutdown / slowdown / max-operating / acoustic thresholds, per-fan speed % and RPM, fan speed range.
*   **PCIe (NVML):** current and maximum generation, width and speed, TX / RX throughput, replay counter.
*   **NVLink:** state, version and remote PCI address per link.
*   **Processes:** running compute and graphics processes with PID, name and GPU memory.

Fields a GPU or driver does not support are left out of the report. Live values (utilization, clocks, power, temperature) are a single sample taken when the tool runs.

## Getting Started

### Prerequisites

*   [Rust Toolchain](https://www.rust-lang.org/tools/install) (Edition 2024)
*   [Make](https://www.gnu.org/software/make/) (optional)
*   Linux. For NVIDIA detail, the NVIDIA driver (which provides `libnvidia-ml.so.1`).

### Build

```bash
cargo build --release
# or
make release
```

The executable is `target/release/gpuinfo-rust`.

## Usage

### Command line

```bash
gpuinfo-rust list        # one line per GPU
gpuinfo-rust info        # full report for every GPU
gpuinfo-rust info 0      # full report for GPU 0
```

`make list` and `make info` do the same through Cargo.

### As an MCP server

Run with no arguments to serve MCP over stdio. Example client configuration (Claude Desktop, `claude_desktop_config.json`):

```json
{
  "mcpServers": {
    "gpuinfo": {
      "command": "/absolute/path/to/gpuinfo-rust/target/release/gpuinfo-rust",
      "args": [],
      "env": {
        "RUST_LOG": "info"
      }
    }
  }
}
```

For Claude Code:

```bash
claude mcp add gpuinfo -- /absolute/path/to/gpuinfo-rust/target/release/gpuinfo-rust
```

Structured JSON logs go to `stderr`.

## Development

*   **Format:** `make fmt`
*   **Lint:** `make clippy`
*   **Test:** `make test`
*   **Check:** `make check`

Source layout:

*   `src/main.rs`: MCP server, tools, CLI, and matching of sysfs GPUs to NVML devices by PCI address.
*   `src/sysfs.rs`: vendor-neutral PCI / DRM / hwmon / amdgpu collection.
*   `src/nvidia.rs`: NVML and `/proc/driver/nvidia` collection.
*   `src/pciids.rs`: PCI vendor, device, subsystem and class names.
*   `src/report.rs`: text report formatting.

## Deployment

*   **Build Docker image:** `make docker-build`
*   **Deploy with Cloud Build:** `make deploy` (requires the Google Cloud SDK)

In a container, NVIDIA detail needs the NVIDIA Container Toolkit (`docker run --gpus all ...`) so that `libnvidia-ml.so.1` and the device nodes are mounted in. MCP servers over stdio are normally run locally rather than as a network service.

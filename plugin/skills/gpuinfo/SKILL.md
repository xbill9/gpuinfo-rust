---
name: gpuinfo
description: Inspect the GPUs on this machine with the gpuinfo MCP server (list_gpus, get_gpu_info). Use whenever a question depends on local GPU hardware or state — which GPU is installed, VRAM total/used/free, driver / CUDA / compute capability versions, clocks, power draw and limits, temperatures and fans, throttle reasons, P-state, PCIe link generation and width, ECC, NVLink, connected monitors, or which processes hold GPU memory — and before choosing a model size, batch size or quantization that has to fit in VRAM, or when debugging slow, throttled or out-of-memory GPU work. Works for NVIDIA (NVML) and any vendor via Linux sysfs; read-only.
---

# gpuinfo

Read-only GPU inspection through the `gpuinfo` MCP server from this plugin. Every value in its report is read from NVML, sysfs or `/proc/driver/nvidia` on the machine Claude Code runs on.

## Workflow

1. **`list_gpus`** — one line per GPU with its number, PCI address, vendor, model, PCI IDs, driver, and (NVIDIA) NVML name and total memory. Start here; the number is what `get_gpu_info` takes.
2. **`get_gpu_info`** with `index` — the full report for one GPU. Omit `index` for every GPU (long on multi-GPU hosts; prefer one index at a time).
3. Answer from the report, quoting the value and the section it came from, e.g. "Memory → Free: 1.81 GiB".

If the MCP tools are not loaded, run the same reports from a shell:

```bash
"${CLAUDE_PLUGIN_ROOT}/bin/gpuinfo-mcp" list
"${CLAUDE_PLUGIN_ROOT}/bin/gpuinfo-mcp" info 0
```

## Quote, don't compute

The report already contains the derived numbers: free memory, the allowed power-limit range, graphics clock steps and range per memory clock, process memory in bytes and GiB. Quote them. Do not add, subtract or convert figures yourself. When a question needs a figure the report does not print (say, VRAM headroom for a model of a given size), show the inputs, do the arithmetic in a shell command, and quote its output.

## Report sections

| Section | Source | What it answers |
| --- | --- | --- |
| Host | kernel, NVML | Driver, NVML and CUDA driver API versions, device nodes, multi-GPU topology |
| PCI (sysfs), Driver | sysfs | Exact model and board vendor, PCI IDs, kernel driver and module version |
| PCIe Link | sysfs | Current vs. max link speed and width, AER error totals |
| Platform, BARs | sysfs | NUMA node, local CPUs, IOMMU group, power state, BAR sizes (resizable BAR) |
| DRM | sysfs | Render nodes, display connectors, connected monitor and preferred mode |
| amdgpu, Sensors | sysfs | AMD VRAM/GTT, busy %, DPM clock levels; hwmon temperatures, power, fans, voltages |
| NVML Identity | NVML | Architecture, compute capability, CUDA cores, UUID, VBIOS, GSP firmware, MIG mode |
| Memory | NVML | VRAM total / used / free / reserved, bus width, BAR1, ECC errors, retired pages |
| Utilization | NVML | GPU and memory-controller %, encoder / decoder %, encoder sessions |
| Clocks (MHz) | NVML | Current and max per clock domain, supported memory clocks and graphics ranges |
| Performance & Modes | NVML | P-state, throttle reasons, compute mode, persistence mode, display state |
| Power, Thermal & Cooling | NVML | Draw vs. limits, energy since driver load, temperature thresholds, fan % and RPM |
| PCIe (NVML), NVLink | NVML | Link generation, width, speed, TX/RX throughput, replay counter, NVLink peers |
| Processes | NVML | PID, process name and GPU memory of every compute and graphics process |

## Reading the report

- **Omitted field** — the GPU or driver does not support it (consumer NVIDIA cards have no ECC or serial number, for example). Say it is not reported; do not guess a value. A field shown as `n/a (NoPermission)` or similar is supported but failed; report the error.
- **Single sample** — utilization, clocks, power, temperature, throughput and the current PCIe link are one reading at call time. Call `get_gpu_info` again for another reading before calling something steady or idle.
- **PCIe link speed drops at idle** — GPUs lower link speed to save power, so a current speed below max on an idle GPU is expected. A **width** below max (e.g. x8 of x16) does not change with load: it is the slot or riser.
- **Throttle reasons** — `none` means no limiter is active. `SW_POWER_CAP` means it hit the power limit (compare Power Draw with Enforced Power Limit); `SW_THERMAL_SLOWDOWN` / `HW_THERMAL_SLOWDOWN` mean temperature; `HW_SLOWDOWN` or `HW_POWER_BRAKE_SLOWDOWN` point at power delivery or an external signal; `GPU_IDLE` just means idle.
- **P-state** — P0 is maximum performance; higher numbers are lower-power states. A busy GPU stuck at a high P-state is worth reporting alongside throttle reasons.
- **Memory → Reserved** is held by the driver and firmware and is not available to applications; Free already excludes it.
- **Processes** lists PIDs across the host; inside a container, PIDs from other namespaces may show a process name of `?`.

## Limits

- Linux only. NVIDIA detail needs the NVIDIA driver (`libnvidia-ml.so.1`); inside containers, the NVIDIA Container Toolkit.
- AMD and Intel detail comes from sysfs only (no ROCm SMI or Level Zero).
- Nothing is changed on the GPU: no clock, power-limit or mode settings.

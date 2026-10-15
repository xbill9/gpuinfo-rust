# gpuinfo plugin for Claude Code

A Claude Code plugin with the `gpuinfo` skill and the [gpuinfo-rust](https://github.com/xbill9/gpuinfo-rust) MCP server. It reports the local GPUs: model, VRAM, driver and CUDA versions, clocks, power, temperatures, throttle reasons, PCIe link, displays and GPU processes.

## Install

```
/plugin marketplace add xbill9/gpuinfo-rust
/plugin install gpuinfo@gpuinfo
```

Or from a shell:

```bash
claude plugin marketplace add xbill9/gpuinfo-rust
claude plugin install gpuinfo@gpuinfo
```

## What you get

- **MCP tools:** `list_gpus` and `get_gpu_info` (optional `index`).
- **Skill:** `gpuinfo`, which tells Claude when to use the tools and how to read the report.

## How the server binary is found

`bin/gpuinfo-mcp` starts the server. It uses the first of these that works:

1. `$GPUINFO_BIN`, if set.
2. `gpuinfo-rust` on `PATH`.
3. A cached prebuilt binary.
4. The prebuilt x86_64 Linux binary from the matching GitHub release, checked against a SHA-256 pinned in the script. It needs glibc 2.34 or newer (Ubuntu 22.04, Debian 12, RHEL 9 or later).
5. `cargo install` from the tagged source, if a Rust toolchain is installed.

## Requirements

Linux. NVIDIA detail needs the NVIDIA driver; AMD and Intel GPUs are reported from sysfs.

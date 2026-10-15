mod nvidia;
mod pciids;
mod report;
mod sysfs;

use anyhow::Result;
use nvml_wrapper::Nvml;
use nvml_wrapper::error::NvmlError;
use rmcp::{
    ServiceExt,
    handler::server::{ServerHandler, tool::ToolRouter, wrapper::Parameters},
    model::{Implementation, ServerCapabilities, ServerInfo},
    schemars, tool, tool_handler, tool_router,
};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use std::sync::{Arc, LazyLock};

use report::{Report, bytes};
use sysfs::PciGpu;

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
struct ListGpusRequest {}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
struct GetGpuInfoRequest {
    /// GPU number as shown by list_gpus. Omit to report every GPU.
    #[serde(default)]
    index: Option<usize>,
}

type Schema = Arc<serde_json::Map<String, serde_json::Value>>;

fn schema_for<T: schemars::JsonSchema>() -> Schema {
    let generator = schemars::generate::SchemaSettings::draft07().into_generator();
    let schema = generator.into_root_schema_for::<T>();
    let mut val = serde_json::to_value(schema).unwrap();
    let obj = val.as_object_mut().unwrap();
    obj.remove("$schema");
    Arc::new(obj.clone())
}

static LIST_GPUS_SCHEMA: LazyLock<Schema> = LazyLock::new(schema_for::<ListGpusRequest>);
static GPU_INFO_SCHEMA: LazyLock<Schema> = LazyLock::new(schema_for::<GetGpuInfoRequest>);

const NVIDIA_VENDOR_ID: u16 = 0x10de;

/// One GPU, seen through sysfs, NVML, or both.
struct Gpu {
    pci: Option<PciGpu>,
    nvml_index: Option<u32>,
}

fn discover(nvml: &Result<Nvml, NvmlError>) -> Vec<Gpu> {
    let mut gpus: Vec<Gpu> = sysfs::discover()
        .into_iter()
        .map(|pci| {
            let nvml_index = match (nvml, pci.bdf()) {
                (Ok(n), Some(bdf)) if pci.vendor_id == NVIDIA_VENDOR_ID => {
                    nvidia::index_for_bdf(n, bdf)
                }
                _ => None,
            };
            Gpu {
                pci: Some(pci),
                nvml_index,
            }
        })
        .collect();

    // NVML devices sysfs did not show (e.g. a container without /sys/bus/pci).
    if let Ok(n) = nvml {
        for i in 0..n.device_count().unwrap_or(0) {
            if !gpus.iter().any(|g| g.nvml_index == Some(i)) {
                gpus.push(Gpu {
                    pci: None,
                    nvml_index: Some(i),
                });
            }
        }
    }
    gpus
}

fn summary(gpu: &Gpu, nvml: &Result<Nvml, NvmlError>) -> String {
    let mut parts = Vec::new();
    if let Some(p) = &gpu.pci {
        parts.push(format!(
            "{}  {} {}  [{:04x}:{:04x}]  driver: {}",
            p.address,
            p.vendor_name(),
            p.device_name(),
            p.vendor_id,
            p.device_id,
            p.driver.as_deref().unwrap_or("none")
        ));
    }
    if let (Some(i), Ok(n)) = (gpu.nvml_index, nvml)
        && let Ok(d) = n.device_by_index(i)
    {
        let name = d.name().unwrap_or_else(|_| "?".into());
        let mem = d
            .memory_info()
            .map(|m| bytes(m.total))
            .unwrap_or_else(|_| "?".into());
        parts.push(format!("NVML {i}: {name}, {mem}"));
    }
    parts.join("  |  ")
}

fn read_trimmed(path: &str) -> Option<String> {
    std::fs::read_to_string(path)
        .ok()
        .map(|s| s.trim().to_string())
}

fn device_nodes() -> String {
    let mut nodes = Vec::new();
    for dir in ["/dev", "/dev/dri", "/dev/accel"] {
        let Ok(rd) = std::fs::read_dir(dir) else {
            continue;
        };
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            let gpu_node = match dir {
                "/dev" => name.starts_with("nvidia") || name == "kfd",
                _ => e.file_type().is_ok_and(|t| !t.is_dir()),
            };
            if gpu_node {
                nodes.push(format!("{dir}/{name}"));
            }
        }
    }
    nodes.sort();
    if nodes.is_empty() {
        "none".into()
    } else {
        nodes.join(", ")
    }
}

fn list_report() -> String {
    let nvml = nvidia::init();
    let gpus = discover(&nvml);
    let mut r = Report::new();
    r.title(&format!("GPUs found: {}", gpus.len()));
    for (i, g) in gpus.iter().enumerate() {
        r.line(format!("GPU {i}: {}", summary(g, &nvml)));
    }
    if gpus.is_empty() {
        r.line("No display controllers or accelerators found on the PCI bus.");
    }
    r.finish()
}

fn info_report(index: Option<usize>) -> String {
    let nvml = nvidia::init();
    let gpus = discover(&nvml);
    let mut r = Report::new();
    r.title("GPU Information Report");

    r.section("Host");
    r.kv(
        "Host Name",
        read_trimmed("/proc/sys/kernel/hostname").unwrap_or_else(|| "<unknown>".into()),
    );
    r.kv(
        "Kernel",
        read_trimmed("/proc/sys/kernel/osrelease").unwrap_or_else(|| "<unknown>".into()),
    );
    r.kv("GPUs Found", gpus.len());
    r.kv("GPU Device Nodes", device_nodes());
    nvidia::render_host(&mut r, &nvml);
    if let Ok(n) = &nvml {
        nvidia::render_topology(&mut r, n);
    }
    r.end_subsection();

    let selected: Vec<(usize, &Gpu)> = match index {
        Some(i) => match gpus.get(i) {
            Some(g) => vec![(i, g)],
            None => {
                r.section("Error");
                r.line(match gpus.len() {
                    0 => format!("No GPU {i}; no GPUs were found."),
                    n => format!(
                        "No GPU {i}; valid numbers are 0 to {} (see list_gpus).",
                        n - 1
                    ),
                });
                return r.finish();
            }
        },
        None => gpus.iter().enumerate().collect(),
    };

    for (i, g) in selected {
        r.section(&format!("GPU {i}: {}", summary(g, &nvml)));
        if let Some(p) = &g.pci {
            sysfs::render(&mut r, p);
            if p.vendor_id == NVIDIA_VENDOR_ID {
                nvidia::render_proc_information(&mut r, &p.address);
            }
        }
        if let (Some(ni), Ok(n)) = (g.nvml_index, &nvml) {
            nvidia::render_device(&mut r, n, ni);
        }
        r.end_subsection();
    }

    r.section("Notes");
    r.line("Fields a GPU or driver does not support are omitted.");
    r.line("Live values (utilization, clocks, power, temperature) are a single sample.");
    r.finish()
}

#[derive(Clone)]
struct GpuInfo {
    tool_router: ToolRouter<Self>,
}

async fn blocking<F: FnOnce() -> String + Send + 'static>(f: F) -> String {
    tokio::task::spawn_blocking(f)
        .await
        .unwrap_or_else(|e| format!("GPU query failed: {e}"))
}

#[tool_router]
impl GpuInfo {
    fn new() -> Self {
        Self {
            tool_router: Self::tool_router(),
        }
    }

    #[tool(
        description = "List every GPU on the host (PCI display controllers and accelerators, plus NVML devices) with PCI address, vendor, model, driver and memory size.",
        input_schema = "LIST_GPUS_SCHEMA.clone()"
    )]
    async fn list_gpus(&self, _params: Parameters<ListGpusRequest>) -> String {
        blocking(list_report).await
    }

    #[tool(
        description = "Detailed GPU report: PCI identity, driver and firmware versions, PCIe link, BARs, display connectors, VRAM and BAR1 memory, ECC, utilization, encoder/decoder, clocks, performance state, throttle reasons, power, temperatures, fans, NVLink, topology and running GPU processes. Uses NVML for NVIDIA and sysfs for AMD/Intel. Pass `index` (from list_gpus) for one GPU, or omit it for all.",
        input_schema = "GPU_INFO_SCHEMA.clone()"
    )]
    async fn get_gpu_info(&self, params: Parameters<GetGpuInfoRequest>) -> String {
        let index = params.0.index;
        blocking(move || info_report(index)).await
    }
}

#[tool_handler]
impl ServerHandler for GpuInfo {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            instructions: Some(
                "A GPU information MCP server. Call list_gpus first, then get_gpu_info for the full detail of one GPU or all of them."
                    .into(),
            ),
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            server_info: Implementation {
                name: env!("CARGO_PKG_NAME").into(),
                version: env!("CARGO_PKG_VERSION").into(),
                ..Default::default()
            },
            ..Default::default()
        }
    }
}

const USAGE: &str = "Usage:
  gpuinfo-rust              run the MCP server on stdio
  gpuinfo-rust list         list GPUs
  gpuinfo-rust info [N]     detailed report for all GPUs, or GPU N";

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("list") => {
            print!("{}", list_report());
            return Ok(());
        }
        Some("info") => {
            let index = match args.get(2) {
                Some(s) => Some(
                    s.parse()
                        .map_err(|_| anyhow::anyhow!("bad GPU number: {s}\n{USAGE}"))?,
                ),
                None => None,
            };
            print!("{}", info_report(index));
            return Ok(());
        }
        Some("-h" | "--help" | "help") => {
            println!("{USAGE}");
            return Ok(());
        }
        _ => {}
    }

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,gpuinfo_rust=debug".into()),
        )
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(std::io::stderr)
                .json(),
        )
        .init();

    tracing::info!("MCP Starting server on stdio");

    let service = GpuInfo::new();
    let transport = rmcp::transport::stdio();
    let server = service.serve(transport).await?;
    server.waiting().await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schemas_have_no_draft_marker() {
        assert!(!LIST_GPUS_SCHEMA.contains_key("$schema"));
        assert!(!GPU_INFO_SCHEMA.contains_key("$schema"));
        let props = GPU_INFO_SCHEMA["properties"].as_object().unwrap();
        assert!(props.contains_key("index"));
    }

    #[test]
    fn report_runs_without_panicking() {
        let text = info_report(None);
        assert!(text.starts_with("GPU Information Report"));
    }
}

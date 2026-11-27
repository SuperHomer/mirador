//! This computer's GPUs: how busy each is, its memory, and who holds it.
//!
//! NVIDIA's `nvidia-smi` is asked first on every OS: the driver installs it
//! on PATH on Linux and Windows, and its CSV output is meant for scripts.
//! Its GPUs are joined to their compute apps by UUID, since the apps query
//! names no index. A reading the card does not give comes back as `[N/A]`
//! or `[Not Supported]`, which becomes None rather than zero.
//!
//! On a Mac with no `nvidia-smi`, `ioreg` lists each `IOAccelerator` with a
//! `PerformanceStatistics` dictionary: the device's utilization and the
//! system memory it has in use. Apple silicon's memory is the system's, so
//! a GPU there has no total of its own. Its processes are the creators of
//! the `AGXDeviceUserClient` connections, which ioreg lists as cheaply
//! (`"pid 650, WindowServer"`); it says nothing of how much memory each
//! holds, so a process there has none.

use std::io::{self, Read};
use std::process::{ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use cmux_protocol::{Gpu, GpuProcess, GpuReport};

use crate::proc::command;

const NVIDIA_GPUS_QUERY: &str =
    "--query-gpu=index,uuid,name,utilization.gpu,memory.used,memory.total,temperature.gpu";
const NVIDIA_APPS_QUERY: &str = "--query-compute-apps=gpu_uuid,pid,process_name,used_memory";
const NVIDIA_FORMAT: &str = "--format=csv,noheader,nounits";
const MIB: f64 = 1024.0 * 1024.0;
/// How long a tool has to answer. A wedged driver (a GPU fallen off the
/// bus) can leave `nvidia-smi` hanging for good, and the pane would hang
/// with it.
const DEADLINE: Duration = Duration::from_secs(5);

pub fn read() -> GpuReport {
    match nvidia() {
        Some(report) => report,
        None if cfg!(target_os = "macos") => apple(),
        None => GpuReport {
            source: "none".into(),
            gpus: Vec::new(),
            error: Some("No nvidia-smi was found on PATH.".into()),
        },
    }
}

/// None when there is no `nvidia-smi` to ask.
fn nvidia() -> Option<GpuReport> {
    let report = |gpus, error| Some(GpuReport { source: "nvidia-smi".into(), gpus, error });
    let text = match run("nvidia-smi", &[NVIDIA_GPUS_QUERY, NVIDIA_FORMAT], DEADLINE) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => return None,
        Err(e) => return report(Vec::new(), Some(e.to_string())),
        Ok(text) => text,
    };
    let mut gpus = parse_nvidia_gpus(&text);
    let error = match run("nvidia-smi", &[NVIDIA_APPS_QUERY, NVIDIA_FORMAT], DEADLINE) {
        Ok(text) => {
            parse_nvidia_apps(&text, &mut gpus);
            None
        }
        Err(e) => Some(e.to_string()),
    };
    report(gpus.into_iter().map(|(_, gpu)| gpu).collect(), error)
}

fn apple() -> GpuReport {
    let report = |gpus, error| GpuReport { source: "apple".into(), gpus, error };
    let text = match run("ioreg", &["-r", "-d", "1", "-c", "IOAccelerator"], DEADLINE) {
        Ok(text) => text,
        Err(e) => return report(Vec::new(), Some(e.to_string())),
    };
    let mut gpus = parse_apple(&text);
    if gpus.is_empty() {
        return report(gpus, Some("ioreg lists no GPU.".into()));
    }
    // The clients name no GPU they are connected to, so they are only
    // placed when there is a single one to place them on.
    if let [gpu] = gpus.as_mut_slice() {
        if let Ok(text) = run("ioreg", &["-r", "-d", "1", "-c", "AGXDeviceUserClient"], DEADLINE) {
            gpu.processes = parse_apple_clients(&text);
        }
    }
    report(gpus, None)
}

/// The program's stdout, or an error saying why there is none: it is not
/// here (`NotFound`), it failed (what it said), or it did not answer by the
/// deadline and was killed. Both pipes are read on threads of their own
/// while it runs: ioreg writes tens of KB, near a pipe's buffer, and a
/// pipe left unread would hold it up as surely as a hang.
fn run(program: &str, args: &[&str], deadline: Duration) -> io::Result<String> {
    let mut child = command(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| io::Error::new(e.kind(), format!("{program}: {e}")))?;
    let stdout = drain(child.stdout.take());
    let stderr = drain(child.stderr.take());
    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if started.elapsed() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            let message = format!("{program} did not answer in {deadline:?}");
            return Err(io::Error::new(io::ErrorKind::TimedOut, message));
        }
        thread::sleep(Duration::from_millis(20));
    };
    let stdout = stdout.join().unwrap_or_default();
    let stderr = stderr.join().unwrap_or_default();
    success(program, status, &stdout, &stderr)
}

fn drain(pipe: Option<impl Read + Send + 'static>) -> thread::JoinHandle<Vec<u8>> {
    thread::spawn(move || {
        let mut bytes = Vec::new();
        if let Some(mut pipe) = pipe {
            let _ = pipe.read_to_end(&mut bytes);
        }
        bytes
    })
}

/// The stdout of a program that ended, or what it said on failing.
fn success(program: &str, status: ExitStatus, stdout: &[u8], stderr: &[u8]) -> io::Result<String> {
    if status.success() {
        return Ok(String::from_utf8_lossy(stdout).into_owned());
    }
    let said = [stderr, stdout]
        .iter()
        .map(|bytes| String::from_utf8_lossy(bytes).trim().to_string())
        .find(|text| !text.is_empty())
        .unwrap_or_else(|| format!("exited with {status}"));
    Err(io::Error::other(format!("{program}: {said}")))
}

/// `nvidia-smi --query-gpu=…` rows, each with its GPU's UUID. A row
/// without an index is skipped.
pub fn parse_nvidia_gpus(csv: &str) -> Vec<(String, Gpu)> {
    csv.lines()
        .filter_map(|line| {
            let fields: Vec<&str> = line.split(", ").map(str::trim).collect();
            // A name with a comma in it spreads over the middle fields.
            let [index, uuid, .., util, used, total, temp] = fields[..] else { return None };
            let name = fields[2..fields.len() - 4].join(", ");
            let gpu = Gpu {
                index: index.parse().ok()?,
                name,
                utilization_percent: number(util),
                memory_used_bytes: mib(used),
                memory_total_bytes: mib(total),
                temperature_c: number(temp),
                processes: Vec::new(),
            };
            Some((uuid.to_string(), gpu))
        })
        .collect()
}

/// `nvidia-smi --query-compute-apps=…` rows, each put on the GPU its UUID
/// names. A row for no GPU listed, or without a pid, is dropped.
pub fn parse_nvidia_apps(csv: &str, gpus: &mut [(String, Gpu)]) {
    for line in csv.lines() {
        let fields: Vec<&str> = line.split(", ").map(str::trim).collect();
        // A process name, often a path, may hold commas of its own.
        let [uuid, pid, .., used] = fields[..] else { continue };
        let Ok(pid) = pid.parse() else { continue };
        let Some((_, gpu)) = gpus.iter_mut().find(|(id, _)| id == uuid) else { continue };
        let name = fields[2..fields.len() - 1].join(", ");
        gpu.processes.push(GpuProcess { pid, name, memory_used_bytes: mib(used) });
    }
}

/// `ioreg -r -d 1 -c IOAccelerator`: a block per GPU, each opening with a
/// `+-o` line, numbered in the order listed.
pub fn parse_apple(ioreg: &str) -> Vec<Gpu> {
    ioreg
        .split("+-o ")
        .skip(1)
        .enumerate()
        .map(|(index, block)| {
            let stats = property(block, "PerformanceStatistics").unwrap_or_default();
            Gpu {
                index: index as u32,
                name: property(block, "model")
                    .or_else(|| property(block, "IOClass"))
                    .map(|name| unquote(&name))
                    .unwrap_or_default(),
                utilization_percent: stat(&stats, "Device Utilization %").and_then(number),
                memory_used_bytes: stat(&stats, "In use system memory").and_then(|n| n.parse().ok()),
                memory_total_bytes: None,
                temperature_c: None,
                processes: Vec::new(),
            }
        })
        .collect()
}

/// `ioreg -r -d 1 -c AGXDeviceUserClient`: each client's creator, once per
/// process however many connections it holds.
pub fn parse_apple_clients(ioreg: &str) -> Vec<GpuProcess> {
    let mut processes: Vec<GpuProcess> = Vec::new();
    for creator in ioreg.lines().filter_map(|line| property(line, "IOUserClientCreator")) {
        let creator = unquote(&creator);
        let Some((pid, name)) = creator.strip_prefix("pid ").and_then(|rest| rest.split_once(", ")) else {
            continue;
        };
        let Ok(pid) = pid.parse() else { continue };
        if processes.iter().all(|p| p.pid != pid) {
            processes.push(GpuProcess { pid, name: name.to_string(), memory_used_bytes: None });
        }
    }
    processes
}

/// The raw value of a `"key" = value` line in an ioreg block.
fn property(block: &str, key: &str) -> Option<String> {
    let prefix = format!("\"{key}\" = ");
    block.lines().find_map(|line| line.trim().strip_prefix(&prefix).map(str::to_string))
}

/// A value in an ioreg dictionary, `{"key"=7,…}`. The key is matched
/// whole: "In use system memory" is not "In use system memory (driver)".
fn stat<'a>(dict: &'a str, key: &str) -> Option<&'a str> {
    let needle = format!("\"{key}\"=");
    let start = dict.find(&needle)? + needle.len();
    let rest = &dict[start..];
    Some(rest[..rest.find([',', '}']).unwrap_or(rest.len())].trim())
}

/// An ioreg string, `"Apple M1"`, or one given as data, `<"AMD Radeon">`.
fn unquote(value: &str) -> String {
    let value = value.trim();
    let value = value.strip_prefix('<').and_then(|v| v.strip_suffix('>')).unwrap_or(value);
    value.trim_matches('"').to_string()
}

fn number(field: &str) -> Option<f64> {
    field.trim().parse().ok().filter(|n: &f64| n.is_finite())
}

fn mib(field: &str) -> Option<u64> {
    number(field).map(|n| (n * MIB) as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    const NVIDIA_GPUS: &str = "\
0, GPU-6f2c1a3e-0000-1111-2222-333344445555, NVIDIA GeForce RTX 4090, 87, 20480, 24564, 71
1, GPU-9b8a7c6d-aaaa-bbbb-cccc-ddddeeeeffff, Tesla T4, [N/A], 1024, 15360, [Not Supported]
";

    const NVIDIA_APPS: &str = "\
GPU-9b8a7c6d-aaaa-bbbb-cccc-ddddeeeeffff, 4242, /usr/bin/python3, 900
GPU-6f2c1a3e-0000-1111-2222-333344445555, 777, C:\\Program Files\\Ollama, Inc\\ollama.exe, [N/A]
GPU-ffffffff-not-a-listed-gpu, 1, ghost, 10
";

    /// `ioreg -r -d 1 -c IOAccelerator` on an M1 MacBook Pro, its long
    /// `IOReportLegend` and the like cut out.
    const IOREG_M1: &str = r#"+-o AGXAcceleratorG13G_B0  <class AGXAcceleratorG13G_B0, id 0x1000006ed, registered, matched, active, busy 0 (92056 ms), retain 78>
    {
      "SchedulerState" = {"Stamps"=(),"BusyWorkQueues"=()}
      "IOMatchedAtBoot" = Yes
      "vendor-id" = <6b100000>
      "AGCInfo" = {"fLastSubmissionPID"=650,"fSubmissionsSinceLastCheck"=0,"fBusyCount"=0}
      "PerformanceStatistics" = {"In use system memory (driver)"=0,"Alloc system memory"=2193539072,"Tiler Utilization %"=7,"recoveryCount"=0,"lastRecoveryTime"=0,"Renderer Utilization %"=7,"TiledSceneBytes"=950272,"Device Utilization %"=7,"SplitSceneCount"=0,"Allocated PB Size"=69337088,"In use system memory"=481558528}
      "model" = "Apple M1"
      "gpu-core-count" = 8
      "IOClass" = "AGXAcceleratorG13G_B0"
      "GPUConfigurationVariable" = {"num_gps"=4,"gpu_gen"=13,"is_sksm"=0,"usc_gen"=2,"num_cores"=8,"num_mgpus"=1,"gpu_var"="G","core_mask_list"=(255),"num_frags"=8}
    }

"#;

    const IOREG_CLIENTS: &str = r#"+-o AGXDeviceUserClient  <class AGXDeviceUserClient, id 0x100000a1c, !registered, !matched, active, busy 0, retain 6>
    {
      "IOUserClientDefaultLocking" = Yes
      "IOUserClientCreator" = "pid 650, WindowServer"
    }

+-o AGXDeviceUserClient  <class AGXDeviceUserClient, id 0x100000a1d, !registered, !matched, active, busy 0, retain 6>
    {
      "IOUserClientCreator" = "pid 650, WindowServer"
    }

+-o AGXDeviceUserClient  <class AGXDeviceUserClient, id 0x100000b2e, !registered, !matched, active, busy 0, retain 6>
    {
      "IOUserClientCreator" = "pid 9001, Mirador Helper, GPU"
    }
"#;

    #[test]
    fn nvidia_gpus_read_with_missing_fields_as_none() {
        let gpus = parse_nvidia_gpus(NVIDIA_GPUS);
        assert_eq!(gpus.len(), 2);
        let (uuid, rtx) = &gpus[0];
        assert_eq!(uuid, "GPU-6f2c1a3e-0000-1111-2222-333344445555");
        assert_eq!(rtx.index, 0);
        assert_eq!(rtx.name, "NVIDIA GeForce RTX 4090");
        assert_eq!(rtx.utilization_percent, Some(87.0));
        assert_eq!(rtx.memory_used_bytes, Some(20480 * 1024 * 1024));
        assert_eq!(rtx.memory_total_bytes, Some(24564 * 1024 * 1024));
        assert_eq!(rtx.temperature_c, Some(71.0));
        let t4 = &gpus[1].1;
        assert_eq!((t4.index, t4.name.as_str()), (1, "Tesla T4"));
        assert_eq!(t4.utilization_percent, None, "[N/A]");
        assert_eq!(t4.temperature_c, None, "[Not Supported]");
        assert_eq!(t4.memory_total_bytes, Some(15360 * 1024 * 1024));
    }

    #[test]
    fn nvidia_apps_go_to_the_gpu_their_uuid_names() {
        let mut gpus = parse_nvidia_gpus(NVIDIA_GPUS);
        parse_nvidia_apps(NVIDIA_APPS, &mut gpus);
        let rtx = &gpus[0].1.processes;
        assert_eq!(
            rtx,
            &[GpuProcess { pid: 777, name: "C:\\Program Files\\Ollama, Inc\\ollama.exe".into(), memory_used_bytes: None }],
            "a comma in the name stays in it"
        );
        let t4 = &gpus[1].1.processes;
        assert_eq!(t4, &[GpuProcess { pid: 4242, name: "/usr/bin/python3".into(), memory_used_bytes: Some(900 * 1024 * 1024) }]);
    }

    #[test]
    fn an_apple_gpu_reads_from_its_performance_statistics() {
        let gpus = parse_apple(IOREG_M1);
        assert_eq!(
            gpus,
            [Gpu {
                index: 0,
                name: "Apple M1".into(),
                utilization_percent: Some(7.0),
                memory_used_bytes: Some(481_558_528),
                memory_total_bytes: None,
                temperature_c: None,
                processes: Vec::new(),
            }]
        );
    }

    #[test]
    fn an_apple_gpu_without_a_model_is_named_by_its_class() {
        let ioreg = "+-o AMDRadeonX6000_AMDNavi14GraphicsAccelerator  <class …>\n    {\n      \"IOClass\" = \"AMDRadeonX6000_AMDNavi14GraphicsAccelerator\"\n    }\n+-o IntelAccelerator  <class …>\n    {\n      \"model\" = <\"Intel UHD Graphics 630\">\n    }\n";
        let gpus = parse_apple(ioreg);
        let names: Vec<_> = gpus.iter().map(|g| (g.index, g.name.as_str())).collect();
        assert_eq!(names, [(0, "AMDRadeonX6000_AMDNavi14GraphicsAccelerator"), (1, "Intel UHD Graphics 630")]);
        assert_eq!(gpus[0].utilization_percent, None);
    }

    #[test]
    fn apple_clients_list_each_process_once() {
        let processes = parse_apple_clients(IOREG_CLIENTS);
        assert_eq!(
            processes,
            [
                GpuProcess { pid: 650, name: "WindowServer".into(), memory_used_bytes: None },
                GpuProcess { pid: 9001, name: "Mirador Helper, GPU".into(), memory_used_bytes: None },
            ]
        );
    }

    #[test]
    fn garbage_gives_no_gpus() {
        for input in ["", "\n\n", "No devices were found", "a, b, c", "x, uuid, name, 1, 2, 3, 4", "+-o", "{\"model\""] {
            assert!(parse_nvidia_gpus(input).is_empty(), "{input:?}");
            assert!(parse_apple_clients(input).is_empty(), "{input:?}");
            let mut gpus = parse_nvidia_gpus(NVIDIA_GPUS);
            parse_nvidia_apps(input, &mut gpus);
            assert!(gpus.iter().all(|(_, g)| g.processes.is_empty()), "{input:?}");
        }
        assert!(parse_apple("").is_empty());
        assert!(parse_apple("No devices were found").is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn a_tool_that_does_not_answer_is_killed_at_the_deadline() {
        let started = Instant::now();
        let error = run("sleep", &["30"], Duration::from_millis(200)).unwrap_err();
        assert!(started.elapsed() < Duration::from_secs(5), "{:?}", started.elapsed());
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        assert_eq!(error.to_string(), "sleep did not answer in 200ms");
    }

    #[cfg(unix)]
    #[test]
    fn a_tool_that_fails_says_why_and_a_missing_one_is_not_found() {
        let error = run("sh", &["-c", "echo broken driver >&2; exit 9"], DEADLINE).unwrap_err();
        assert_eq!(error.to_string(), "sh: broken driver");
        let missing = run("mirador-no-such-tool", &[], DEADLINE).unwrap_err();
        assert_eq!(missing.kind(), io::ErrorKind::NotFound);
        let big = run("sh", &["-c", "head -c 200000 /dev/zero"], DEADLINE).unwrap();
        assert_eq!(big.len(), 200_000, "a full pipe is read, not waited on");
    }

    /// What this machine reports: `cargo test -p cmux-core gpu -- --nocapture`.
    #[test]
    fn read_never_panics() {
        let report = read();
        eprintln!("{report:#?}");
        assert!(["nvidia-smi", "apple", "none"].contains(&report.source.as_str()));
        if report.source == "none" {
            assert!(report.gpus.is_empty() && report.error.is_some());
        }
    }
}

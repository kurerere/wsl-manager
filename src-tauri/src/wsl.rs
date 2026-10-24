use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::{mpsc, LazyLock, Mutex};
use std::thread;
use std::time::{Duration, Instant};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const READ_TIMEOUT: Duration = Duration::from_secs(10);
/// 新会话没有上一次的快照，先等这么久再采第二次来计算 CPU
const WARMUP: Duration = Duration::from_millis(300);

/// 会话启动时执行一次，获取不会变化的参数。
const INIT_CMD: &str = "exec 2>/dev/null; \
echo '@@NPROC'; nproc || grep -c ^processor /proc/cpuinfo; \
echo '@@PAGESIZE'; getconf PAGESIZE || echo 4096; \
echo '@@END'\n";

/// 每次刷新在发行版内执行的采样命令。
///
/// WSL2 的所有发行版共享同一个虚拟机内核，所以 /proc/stat 和 /proc/meminfo
/// 反映的是整个 VM；而每个发行版有独立的 PID 命名空间，/proc/<pid> 只包含
/// 本发行版的进程，据此统计单个发行版的 CPU 和内存占用。
const SAMPLE_CMD: &str = "echo '@@CPU'; head -n1 /proc/stat; \
echo '@@PROC'; cat /proc/[0-9]*/stat; echo '@@STATM'; grep -H '' /proc/[0-9]*/statm; \
echo '@@MEMINFO'; cat /proc/meminfo; \
echo '@@DF'; df -kP / | tail -n1; \
echo '@@END'\n";

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct DistroStats {
    /// 占整个 WSL VM CPU 容量的百分比（两次刷新之间的平均值）
    pub cpu_percent: f64,
    /// 本发行版所有进程的私有内存（RSS 减去共享/文件页）之和，字节。
    /// 共享的文件页属于页缓存，不计入，避免与缓存及其它发行版重复计算。
    pub mem_used: u64,
    /// 所有进程 RSS 之和，字节
    pub mem_rss: u64,
    pub process_count: u32,
    pub disk_total: u64,
    pub disk_used: u64,
    pub disk_avail: u64,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct VmStats {
    pub cpu_percent: f64,
    pub cpu_cores: u32,
    pub mem_total: u64,
    pub mem_available: u64,
    pub mem_free: u64,
    /// 页缓存 + 可回收 slab，可以通过释放缓存还给 Windows
    pub mem_cache: u64,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct HostMemory {
    pub total: u64,
    pub available: u64,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Distro {
    pub name: String,
    pub running: bool,
    pub version: u8,
    pub is_default: bool,
    pub vhdx_path: Option<String>,
    pub compact_blocker: Option<String>,
    /// VHDX 文件在 Windows 磁盘上实际占用的大小，字节
    pub vhdx_size: Option<u64>,
    pub stats: Option<DistroStats>,
    pub error: Option<String>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Overview {
    pub distros: Vec<Distro>,
    pub vm: Option<VmStats>,
    pub host: Option<HostMemory>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompactResult {
    pub before: u64,
    pub after: u64,
}

/// Windows 物理内存
#[cfg(windows)]
fn host_memory() -> Option<HostMemory> {
    use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    let mut m: MEMORYSTATUSEX = unsafe { std::mem::zeroed() };
    m.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
    (unsafe { GlobalMemoryStatusEx(&mut m) } != 0).then(|| HostMemory {
        total: m.ullTotalPhys,
        available: m.ullAvailPhys,
    })
}

#[cfg(not(windows))]
fn host_memory() -> Option<HostMemory> {
    None
}

fn wsl() -> Command {
    let mut cmd = Command::new("wsl.exe");
    cmd.env("WSL_UTF8", "1");
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);
    cmd
}

/// wsl.exe 的管理命令默认输出 UTF-16LE，设置 WSL_UTF8 后为 UTF-8，两种都兼容。
fn decode(bytes: &[u8]) -> String {
    if bytes.contains(&0) {
        let units: Vec<u16> = bytes
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        String::from_utf16_lossy(&units)
    } else {
        String::from_utf8_lossy(bytes).into_owned()
    }
    .trim_start_matches('\u{feff}')
    .to_string()
}

fn run_wsl(args: &[&str]) -> Result<String, String> {
    let out = wsl().args(args).output().map_err(|e| format!("无法执行 wsl.exe: {e}"))?;
    if out.status.success() {
        Ok(decode(&out.stdout))
    } else {
        let msg = decode(&out.stderr);
        let msg = if msg.trim().is_empty() { decode(&out.stdout) } else { msg };
        Err(msg.trim().to_string())
    }
}

fn parse_wsl_version(output: &str) -> Option<(u32, u32, u32)> {
    output.lines().next()?.split_whitespace().find_map(|token| {
        let parts: Vec<_> = token.split('.').collect();
        if !(3..=4).contains(&parts.len()) || parts.iter().any(|part| !part.bytes().all(|b| b.is_ascii_digit())) {
            return None;
        }
        Some((parts[0].parse().ok()?, parts[1].parse().ok()?, parts[2].parse().ok()?))
    })
}

fn compact_version_supported(version: (u32, u32, u32)) -> bool {
    version >= (3, 0, 1)
}

fn compact_supported() -> bool {
    run_wsl(&["--version"])
        .ok()
        .and_then(|output| parse_wsl_version(&output))
        .is_some_and(compact_version_supported)
}

const FILE_ATTRIBUTE_SPARSE_FILE: u32 = 0x200;
const FILE_ATTRIBUTE_COMPRESSED: u32 = 0x800;
const FILE_ATTRIBUTE_ENCRYPTED: u32 = 0x4000;

fn compact_blocker(name: &str, attributes: u32) -> Option<String> {
    if attributes & FILE_ATTRIBUTE_SPARSE_FILE != 0 {
        Some(format!(
            "{name} 的虚拟磁盘是稀疏文件（已启用稀疏 VHD），WSL 不支持压缩这类磁盘。稀疏 VHD 会自动回收已释放的空间，无需手动压缩。"
        ))
    } else if attributes & FILE_ATTRIBUTE_COMPRESSED != 0 {
        Some(format!(
            "{name} 的虚拟磁盘启用了 NTFS 压缩，WSL 不支持压缩这类磁盘。"
        ))
    } else if attributes & FILE_ATTRIBUTE_ENCRYPTED != 0 {
        Some(format!(
            "{name} 的虚拟磁盘启用了 NTFS 加密（EFS），WSL 不支持压缩这类磁盘。"
        ))
    } else {
        None
    }
}

#[cfg(windows)]
fn compact_blocker_from_metadata(name: &str, metadata: &std::fs::Metadata) -> Option<String> {
    use std::os::windows::fs::MetadataExt;
    compact_blocker(name, metadata.file_attributes())
}

#[cfg(not(windows))]
fn compact_blocker_from_metadata(_name: &str, _metadata: &std::fs::Metadata) -> Option<String> {
    None
}

#[cfg(windows)]
fn compact_blocker_for_path(name: &str, path: &str) -> Option<String> {
    let metadata = std::fs::metadata(path).ok()?;
    compact_blocker_from_metadata(name, &metadata)
}

#[cfg(not(windows))]
fn compact_blocker_for_path(_name: &str, _path: &str) -> Option<String> {
    None
}

#[cfg(windows)]
fn allocated_size(path: &str) -> Option<u64> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::{
        Foundation::{GetLastError, SetLastError},
        Storage::FileSystem::{GetCompressedFileSizeW, INVALID_FILE_SIZE},
    };

    let path: Vec<u16> = std::ffi::OsStr::new(path).encode_wide().chain(Some(0)).collect();
    let mut high = 0;
    unsafe {
        SetLastError(0);
        let low = GetCompressedFileSizeW(path.as_ptr(), &mut high);
        if low == INVALID_FILE_SIZE && GetLastError() != 0 {
            return None;
        }
        Some((u64::from(high) << 32) | u64::from(low))
    }
}

#[cfg(not(windows))]
fn allocated_size(_path: &str) -> Option<u64> {
    None
}

fn vhdx_usage(path: &str, metadata: &std::fs::Metadata) -> u64 {
    allocated_size(path).unwrap_or(metadata.len())
}

struct ListEntry {
    name: String,
    version: u8,
    is_default: bool,
    vhdx_path: Option<String>,
}

/// 从注册表读取已注册的发行版，比调用 `wsl -l -v` 快得多，也不受系统语言影响。
#[cfg(windows)]
fn list_entries() -> Vec<ListEntry> {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;

    /// LXSS_DISTRO_FLAGS_VM_MODE：置位表示 WSL2
    const FLAG_WSL2: u32 = 0x8;

    let Ok(lxss) = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey(r"Software\Microsoft\Windows\CurrentVersion\Lxss")
    else {
        return Vec::new();
    };
    let default: String = lxss.get_value("DefaultDistribution").unwrap_or_default();
    let mut entries: Vec<ListEntry> = lxss
        .enum_keys()
        .flatten()
        .filter_map(|guid| {
            let key = lxss.open_subkey(&guid).ok()?;
            let name: String = key.get_value("DistributionName").ok()?;
            let flags: u32 = key.get_value("Flags").unwrap_or(FLAG_WSL2);
            let vhdx_path = key.get_value::<String, _>("BasePath").ok().map(|base| {
                let file: String = key.get_value("VhdFileName").unwrap_or_else(|_| "ext4.vhdx".into());
                let base = base.trim_start_matches(r"\\?\");
                std::path::Path::new(base).join(file).to_string_lossy().into_owned()
            });
            Some(ListEntry {
                name,
                version: if flags & FLAG_WSL2 != 0 { 2 } else { 1 },
                is_default: guid.eq_ignore_ascii_case(&default),
                vhdx_path,
            })
        })
        .collect();
    // 与 `wsl -l` 一致：默认发行版在前
    entries.sort_by(|a, b| {
        b.is_default
            .cmp(&a.is_default)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    entries
}

#[cfg(not(windows))]
fn list_entries() -> Vec<ListEntry> {
    Vec::new()
}

pub(crate) fn running_set() -> HashSet<String> {
    // 没有运行中的发行版时 wsl.exe 返回非零退出码
    run_wsl(&["--list", "--running", "--quiet"])
        .map(|t| t.lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect())
        .unwrap_or_default()
}

fn sections(lines: &[String]) -> HashMap<&str, Vec<&str>> {
    let mut map: HashMap<&str, Vec<&str>> = HashMap::new();
    let mut current = "";
    for line in lines {
        if let Some(tag) = line.strip_prefix("@@") {
            current = tag.trim();
            map.entry(current).or_default();
        } else if !current.is_empty() {
            map.entry(current).or_default().push(line);
        }
    }
    map
}

/// 返回 (total, idle) jiffies
fn parse_cpu_line(line: &str) -> Option<(u64, u64)> {
    let nums: Vec<u64> = line
        .split_whitespace()
        .skip(1)
        .take(8) // user nice system idle iowait irq softirq steal；guest 已计入 user
        .filter_map(|v| v.parse().ok())
        .collect();
    if nums.len() < 5 {
        return None;
    }
    Some((nums.iter().sum(), nums[3] + nums[4]))
}

/// 解析 /proc/<pid>/stat，返回 pid -> utime + stime
fn parse_procs(lines: &[&str]) -> HashMap<u32, u64> {
    let mut map = HashMap::new();
    for line in lines {
        // comm 字段可能包含空格和括号，以最后一个 ')' 为界
        let (Some(open), Some(close)) = (line.find(" ("), line.rfind(')')) else { continue };
        let Ok(pid) = line[..open].trim().parse::<u32>() else { continue };
        let rest: Vec<&str> = line[close + 1..].split_whitespace().collect();
        // rest[0] 是第 3 个字段 (state)，utime/stime 分别是第 14/15 个字段
        if rest.len() < 13 {
            continue;
        }
        let num = |i: usize| rest[i].parse::<u64>().unwrap_or(0);
        map.insert(pid, num(11) + num(12));
    }
    map
}

/// 解析 `grep -H '' /proc/*/statm` 的输出，返回 (rss 页数, 私有页数) 之和
fn parse_statm(lines: &[&str]) -> (u64, u64) {
    lines
        .iter()
        .filter_map(|l| {
            // /proc/<pid>/statm:size resident shared text lib data dt
            let mut f = l.split_once(':')?.1.split_whitespace().skip(1);
            let resident: u64 = f.next()?.parse().ok()?;
            let shared: u64 = f.next()?.parse().ok()?;
            Some((resident, resident.saturating_sub(shared)))
        })
        .fold((0, 0), |a, b| (a.0 + b.0, a.1 + b.1))
}

fn meminfo_kb(lines: &[&str], key: &str) -> u64 {
    lines
        .iter()
        .find_map(|l| l.strip_prefix(key)?.strip_prefix(':'))
        .and_then(|v| v.split_whitespace().next()?.parse().ok())
        .unwrap_or(0)
}

/// 一次采样的原始数据
struct Snapshot {
    cpu_total: u64,
    cpu_idle: u64,
    procs: HashMap<u32, u64>,
    /// (rss 页数, 私有页数)
    statm: (u64, u64),
    mem_total: u64,
    mem_available: u64,
    mem_free: u64,
    mem_cache: u64,
    disk: (u64, u64, u64),
}

fn parse_snapshot(lines: &[String]) -> Result<Snapshot, String> {
    let s = sections(lines);
    let empty = Vec::new();
    let get = |k: &str| s.get(k).unwrap_or(&empty);

    let (cpu_total, cpu_idle) = get("CPU")
        .first()
        .and_then(|l| parse_cpu_line(l))
        .ok_or("无法读取 /proc/stat")?;
    let mem = get("MEMINFO");
    // df -kP: Filesystem 1024-blocks Used Available Capacity Mounted
    let df: Vec<&str> = get("DF").first().map(|l| l.split_whitespace().collect()).unwrap_or_default();
    let df_kb = |from_end: usize| {
        df.len()
            .checked_sub(from_end)
            .and_then(|i| df[i].parse::<u64>().ok())
            .unwrap_or(0)
            * 1024
    };
    Ok(Snapshot {
        cpu_total,
        cpu_idle,
        procs: parse_procs(get("PROC")),
        statm: parse_statm(get("STATM")),
        mem_total: meminfo_kb(mem, "MemTotal") * 1024,
        mem_available: meminfo_kb(mem, "MemAvailable") * 1024,
        mem_free: meminfo_kb(mem, "MemFree") * 1024,
        mem_cache: (meminfo_kb(mem, "Buffers") + meminfo_kb(mem, "Cached") + meminfo_kb(mem, "SReclaimable"))
            * 1024,
        disk: (df_kb(5), df_kb(4), df_kb(3)),
    })
}

/// 由前后两次快照计算统计数据
fn compute(prev: &Snapshot, cur: &Snapshot, cores: u32, page_size: u64) -> (DistroStats, VmStats) {
    let dt = cur.cpu_total.saturating_sub(prev.cpu_total).max(1) as f64;
    let idle = cur.cpu_idle.saturating_sub(prev.cpu_idle) as f64;
    // 期间新启动的进程没有上次的值，按其全部 CPU 时间计入
    let busy: u64 = cur
        .procs
        .iter()
        .map(|(pid, t)| t.saturating_sub(prev.procs.get(pid).copied().unwrap_or(0)))
        .sum();
    (
        DistroStats {
            cpu_percent: (busy as f64 / dt * 100.0).clamp(0.0, 100.0),
            mem_used: cur.statm.1 * page_size,
            mem_rss: cur.statm.0 * page_size,
            process_count: cur.procs.len() as u32,
            disk_total: cur.disk.0,
            disk_used: cur.disk.1,
            disk_avail: cur.disk.2,
        },
        VmStats {
            cpu_percent: ((1.0 - idle / dt) * 100.0).clamp(0.0, 100.0),
            cpu_cores: cores,
            mem_total: cur.mem_total,
            mem_available: cur.mem_available,
            mem_free: cur.mem_free,
            mem_cache: cur.mem_cache,
        },
    )
}

/// 在发行版里常驻的 sh 进程。复用它采样，避免每次刷新都启动 wsl.exe。
struct Session {
    child: Child,
    stdin: ChildStdin,
    lines: mpsc::Receiver<String>,
    /// 初始化命令的输出还没读取
    init_pending: bool,
    cores: u32,
    page_size: u64,
    prev: Option<Snapshot>,
}

impl Session {
    fn start(name: &str) -> Result<Self, String> {
        let mut child = wsl()
            .args(["--distribution", name, "--exec", "sh"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("无法执行 wsl.exe: {e}"))?;
        let stdin = child.stdin.take().ok_or("无法打开 stdin")?;
        let stdout = child.stdout.take().ok_or("无法打开 stdout")?;

        // 后台线程逐行转发输出，主流程用 recv_timeout 实现超时
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            let mut buf = Vec::new();
            loop {
                buf.clear();
                match reader.read_until(b'\n', &mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {
                        let line = String::from_utf8_lossy(&buf).trim_end().to_string();
                        if tx.send(line).is_err() {
                            break;
                        }
                    }
                }
            }
        });

        let mut session = Session {
            child,
            stdin,
            lines: rx,
            init_pending: true,
            cores: 0,
            page_size: 4096,
            prev: None,
        };
        session.send(INIT_CMD)?;
        Ok(session)
    }

    fn send(&mut self, cmd: &str) -> Result<(), String> {
        self.stdin
            .write_all(cmd.as_bytes())
            .and_then(|_| self.stdin.flush())
            .map_err(|_| "会话已断开".to_string())
    }

    fn read_block(&mut self, deadline: Instant) -> Result<Vec<String>, String> {
        let mut lines = Vec::new();
        loop {
            let wait = deadline.saturating_duration_since(Instant::now());
            match self.lines.recv_timeout(wait) {
                Ok(line) if line == "@@END" => return Ok(lines),
                Ok(line) => lines.push(line),
                Err(mpsc::RecvTimeoutError::Timeout) => return Err("采样超时".into()),
                Err(mpsc::RecvTimeoutError::Disconnected) => return Err("会话已断开".into()),
            }
        }
    }

    fn read_snapshot(&mut self, deadline: Instant) -> Result<Snapshot, String> {
        if self.init_pending {
            let lines = self.read_block(deadline)?;
            let s = sections(&lines);
            let first = |k: &str| s.get(k).and_then(|v| v.first()).map(|v| v.trim());
            self.cores = first("NPROC").and_then(|v| v.parse().ok()).unwrap_or(0);
            self.page_size = first("PAGESIZE").and_then(|v| v.parse().ok()).unwrap_or(4096);
            self.init_pending = false;
        }
        parse_snapshot(&self.read_block(deadline)?)
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// 发行版名 -> 常驻会话
static SESSIONS: LazyLock<Mutex<HashMap<String, Session>>> = LazyLock::new(Default::default);
static COMPACTING: LazyLock<Mutex<Option<String>>> = LazyLock::new(Default::default);

struct CompactGuard;

impl Drop for CompactGuard {
    fn drop(&mut self) {
        *COMPACTING.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }
}

pub fn close_sessions() {
    SESSIONS.lock().unwrap_or_else(|e| e.into_inner()).clear();
}

/// 先给所有会话发采样命令，再统一读取，各发行版并行执行
fn sample_all<'a>(
    sessions: &mut HashMap<String, Session>,
    names: impl Iterator<Item = &'a String> + Clone,
) -> HashMap<String, Result<Snapshot, String>> {
    let mut results = HashMap::new();
    for name in names.clone() {
        if let Some(Err(e)) = sessions.get_mut(name).map(|s| s.send(SAMPLE_CMD)) {
            results.insert(name.clone(), Err(e));
        }
    }
    let deadline = Instant::now() + READ_TIMEOUT;
    for name in names {
        if results.contains_key(name) {
            continue;
        }
        if let Some(s) = sessions.get_mut(name) {
            results.insert(name.clone(), s.read_snapshot(deadline));
        }
    }
    results
}

pub fn overview() -> Result<Overview, String> {
    let entries = list_entries();
    let running = running_set();

    let mut sessions = SESSIONS.lock().unwrap_or_else(|e| e.into_inner());
    let compacting = COMPACTING.lock().unwrap_or_else(|e| e.into_inner()).clone();
    // 已停止的发行版关闭会话，新运行的发行版建立会话
    sessions.retain(|name, _| running.contains(name) && compacting.as_deref() != Some(name));
    let mut errors: HashMap<String, String> = HashMap::new();
    for name in &running {
        if compacting.as_deref() == Some(name) {
            continue;
        }
        if !sessions.contains_key(name) {
            match Session::start(name) {
                Ok(s) => {
                    sessions.insert(name.clone(), s);
                }
                Err(e) => {
                    errors.insert(name.clone(), e);
                }
            }
        }
    }

    let names: Vec<String> = sessions.keys().cloned().collect();
    let mut snapshots = sample_all(&mut sessions, names.iter());

    // 新会话没有上一次快照，短暂等待后再采一次
    let warmup: Vec<String> = names
        .iter()
        .filter(|n| sessions[*n].prev.is_none() && matches!(snapshots.get(*n), Some(Ok(_))))
        .cloned()
        .collect();
    if !warmup.is_empty() {
        for name in &warmup {
            if let Some(Ok(snap)) = snapshots.remove(name) {
                sessions.get_mut(name).unwrap().prev = Some(snap);
            }
        }
        thread::sleep(WARMUP);
        snapshots.extend(sample_all(&mut sessions, warmup.iter()));
    }

    let mut stats: HashMap<String, (DistroStats, VmStats)> = HashMap::new();
    for (name, result) in snapshots {
        match result {
            Ok(cur) => {
                let s = sessions.get_mut(&name).unwrap();
                if let Some(prev) = &s.prev {
                    stats.insert(name.clone(), compute(prev, &cur, s.cores, s.page_size));
                }
                s.prev = Some(cur);
            }
            Err(e) => {
                // 出错的会话丢弃，下次刷新重建
                sessions.remove(&name);
                errors.insert(name, e);
            }
        }
    }
    drop(sessions);

    let mut vm = None;
    let distros = entries
        .into_iter()
        .map(|e| {
            let (stats, vm_stats) = stats.remove(&e.name).unzip();
            if vm.is_none() {
                vm = vm_stats;
            }
            let metadata = e
                .vhdx_path
                .as_ref()
                .and_then(|path| std::fs::metadata(path).ok());
            let vhdx_size = e.vhdx_path.as_ref().zip(metadata.as_ref())
                .map(|(path, metadata)| vhdx_usage(path, metadata));
            let compact_blocker = metadata
                .as_ref()
                .and_then(|metadata| compact_blocker_from_metadata(&e.name, metadata));
            Distro {
                running: running.contains(&e.name),
                error: errors.remove(&e.name),
                name: e.name,
                version: e.version,
                is_default: e.is_default,
                vhdx_path: e.vhdx_path,
                compact_blocker,
                vhdx_size,
                stats,
            }
        })
        .collect();

    Ok(Overview { distros, vm, host: host_memory() })
}

/// 释放 WSL 虚拟机的页缓存。缓存是整个 VM 共享的，在任意一个运行中的发行版里以 root 执行即可，
/// 释放出的内存会通过 free page reporting 归还给 Windows。
pub fn drop_caches() -> Result<(), String> {
    let running = running_set();
    let name = list_entries()
        .into_iter()
        .map(|e| e.name)
        .find(|n| running.contains(n))
        .ok_or("没有运行中的发行版")?;
    run_wsl(&[
        "--distribution",
        &name,
        "--user",
        "root",
        "--exec",
        "sh",
        "-c",
        "sync; echo 3 > /proc/sys/vm/drop_caches",
    ])
    .map(|_| ())
}

pub fn compact(name: &str) -> Result<CompactResult, String> {
    {
        let mut compacting = COMPACTING.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(other) = compacting.as_ref() {
            return Err(format!("正在压缩 {other}，请稍后再试"));
        }
        *compacting = Some(name.to_string());
    }
    let _guard = CompactGuard;

    let entry = list_entries()
        .into_iter()
        .find(|entry| entry.name == name)
        .ok_or_else(|| format!("未找到发行版 {name}"))?;
    let path = entry.vhdx_path.ok_or_else(|| format!("未找到 {name} 的 VHDX 文件路径"))?;
    if let Some(blocker) = compact_blocker_for_path(name, &path) {
        return Err(blocker);
    }
    if entry.version != 2 {
        return Err("仅支持压缩 WSL 2 发行版".into());
    }
    if !compact_supported() {
        return Err("压缩磁盘需要 WSL 3.0.1 或更高版本，请先运行 wsl --update".into());
    }
    let before_metadata =
        std::fs::metadata(&path).map_err(|e| format!("读取 VHDX 文件大小失败：{e}"))?;
    let before = vhdx_usage(&path, &before_metadata);

    if running_set().contains(name) {
        SESSIONS.lock().unwrap_or_else(|e| e.into_inner()).remove(name);
        terminate(name)?;
        let stopped_at = Instant::now();
        loop {
            if !running_set().contains(name) {
                break;
            }
            if stopped_at.elapsed() >= Duration::from_secs(10) {
                return Err(format!("等待发行版 {name} 停止超时"));
            }
            thread::sleep(Duration::from_millis(300));
        }
    }

    run_wsl(&["--manage", name, "--compact"])?;
    let after_metadata =
        std::fs::metadata(&path).map_err(|e| format!("读取 VHDX 文件大小失败：{e}"))?;
    let after = vhdx_usage(&path, &after_metadata);
    Ok(CompactResult { before, after })
}

pub fn terminate(name: &str) -> Result<(), String> {
    run_wsl(&["--terminate", name]).map(|_| ())
}

pub fn shutdown() -> Result<(), String> {
    run_wsl(&["--shutdown"]).map(|_| ())
}

pub fn set_default(name: &str) -> Result<(), String> {
    run_wsl(&["--set-default", name]).map(|_| ())
}

/// 在新终端窗口中打开发行版，优先使用 Windows Terminal。
pub fn open_terminal(name: &str) -> Result<(), String> {
    if Command::new("wt.exe").args(["wsl.exe", "-d", name]).spawn().is_ok() {
        return Ok(());
    }
    let mut cmd = Command::new("cmd.exe");
    cmd.args(["/c", "start", "", "wsl.exe", "-d", name]);
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);
    cmd.spawn()
        .map(|_| ())
        .map_err(|e| format!("无法打开终端: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_chinese_wsl_version() {
        assert_eq!(parse_wsl_version("WSL 版本: 3.0.3.0"), Some((3, 0, 3)));
    }

    #[test]
    fn parses_english_wsl_version() {
        assert_eq!(parse_wsl_version("WSL version: 2.6.1.0"), Some((2, 6, 1)));
    }

    #[test]
    fn ignores_unrecognized_wsl_version() {
        assert_eq!(parse_wsl_version("not a version\nWSL version: 3.0.3.0"), None);
    }

    #[test]
    fn checks_compaction_version_requirement() {
        assert!(!compact_version_supported((2, 6, 1)));
        assert!(compact_version_supported((3, 0, 1)));
        assert!(compact_version_supported((3, 0, 3)));
    }

    #[test]
    fn detects_sparse_disk_compaction_blocker() {
        assert_eq!(
            compact_blocker("AlpineStore", 0x200),
            Some("AlpineStore 的虚拟磁盘是稀疏文件（已启用稀疏 VHD），WSL 不支持压缩这类磁盘。稀疏 VHD 会自动回收已释放的空间，无需手动压缩。".into())
        );
    }

    #[test]
    fn detects_compressed_disk_compaction_blocker() {
        assert_eq!(
            compact_blocker("AlpineStore", 0x800),
            Some("AlpineStore 的虚拟磁盘启用了 NTFS 压缩，WSL 不支持压缩这类磁盘。".into())
        );
    }

    #[test]
    fn detects_encrypted_disk_compaction_blocker() {
        assert_eq!(
            compact_blocker("AlpineStore", 0x4000),
            Some("AlpineStore 的虚拟磁盘启用了 NTFS 加密（EFS），WSL 不支持压缩这类磁盘。".into())
        );
    }

    #[test]
    fn ignores_plain_archive_attribute_for_compaction() {
        assert_eq!(compact_blocker("AlpineStore", 0x20), None);
    }
}

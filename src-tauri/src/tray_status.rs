use std::time::{Duration, Instant};

use crate::cache_schedule::Status;
use crate::wsl::VmSample;

const TOOLTIP_LIMIT: usize = 127;

pub fn tooltip_text(
    vm: Option<&VmSample>,
    running: usize,
    schedule: &Status,
    now: Instant,
    update: Option<&str>,
) -> String {
    let mut lines = vec!["WSL Manager".to_string()];
    if running == 0 {
        lines.push("WSL 未运行".into());
    } else {
        if let Some(vm) = vm {
            let used = vm.total.saturating_sub(vm.free);
            let mut memory = format!(
                "内存 {} / {} · 缓存 {}",
                format_bytes(used),
                format_bytes(vm.total),
                format_bytes(vm.cache)
            );
            let age = now.saturating_duration_since(vm.at);
            if age > Duration::from_secs(60) {
                memory.push_str(&format!(" ({} 分钟前)", (age.as_secs() / 60).max(1)));
            }
            lines.push(memory);
        }
        lines.push(format!("{running} 个发行版运行中"));
    }
    lines.push(schedule_text(schedule));
    if let Some(version) = update {
        lines.push(format!("有新版本 v{version}"));
    }
    truncate_tooltip(&lines.join("\n"))
}

fn schedule_text(schedule: &Status) -> String {
    let mut text = if schedule.busy {
        "正在释放缓存…".into()
    } else if schedule.minutes == 0 {
        "定时释放：关闭".into()
    } else if schedule.waiting {
        "定时释放：等待 WSL 运行".into()
    } else if schedule.remaining < 60 {
        "定时释放：不到 1 分钟".into()
    } else {
        format!("定时释放：{} 分钟后", schedule.remaining.div_ceil(60))
    };
    if !schedule.busy && schedule.error.is_some() {
        text.push_str("（上次失败）");
    }
    text
}

fn format_bytes(bytes: u64) -> String {
    if bytes == 0 {
        return "0 B".into();
    }
    let units = ["B", "KB", "MB", "GB", "TB"];
    let index = ((bytes as f64).ln() / 1024_f64.ln()).floor() as usize;
    let index = index.min(units.len() - 1);
    let value = bytes as f64 / 1024_f64.powi(index as i32);
    let value = if value >= 100.0 {
        format!("{value:.0}")
    } else {
        format!("{value:.1}")
    };
    format!("{value} {}", units[index])
}

fn truncate_tooltip(text: &str) -> String {
    let mut result = String::new();
    let mut units = 0;
    for ch in text.chars() {
        let length = ch.len_utf16();
        if units + length > TOOLTIP_LIMIT {
            break;
        }
        result.push(ch);
        units += length;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn schedule(minutes: u32, remaining: u64) -> Status {
        Status {
            minutes,
            remaining,
            busy: false,
            waiting: false,
            completed: 0,
            error: None,
        }
    }

    fn vm(at: Instant) -> VmSample {
        VmSample {
            at,
            total: 12 * 1024_u64.pow(3),
            free: 12 * 1024_u64.pow(3) - 6 * 1024_u64.pow(3) - 1024_u64.pow(3) / 5,
            cache: 3 * 1024_u64.pow(3) + 1024_u64.pow(3) / 10,
        }
    }

    #[test]
    fn formats_bytes_using_binary_units() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(1023), "1023 B");
        assert_eq!(format_bytes(1536 * 1024 * 1024), "1.5 GB");
        assert_eq!(format_bytes(100 * 1024_u64.pow(3)), "100 GB");
    }

    #[test]
    fn formats_running_memory_and_schedule_countdown() {
        let now = Instant::now();
        let text = tooltip_text(Some(&vm(now)), 2, &schedule(20, 700), now, None);
        assert_eq!(
            text,
            "WSL Manager\n内存 6.2 GB / 12.0 GB · 缓存 3.1 GB\n2 个发行版运行中\n定时释放：12 分钟后"
        );
    }

    #[test]
    fn appends_available_update_version() {
        let now = Instant::now();
        assert_eq!(
            tooltip_text(None, 0, &schedule(0, 0), now, Some("0.3.0")),
            "WSL Manager\nWSL 未运行\n定时释放：关闭\n有新版本 v0.3.0"
        );
    }

    #[test]
    fn annotates_stale_memory_samples() {
        let now = Instant::now();
        let text = tooltip_text(
            Some(&vm(now - Duration::from_secs(181))),
            1,
            &schedule(0, 0),
            now,
            None,
        );
        assert!(text.contains(" (3 分钟前)"));
    }

    #[test]
    fn formats_not_running_and_running_without_a_sample() {
        let now = Instant::now();
        assert_eq!(
            tooltip_text(None, 0, &schedule(0, 0), now, None),
            "WSL Manager\nWSL 未运行\n定时释放：关闭"
        );
        assert_eq!(
            tooltip_text(None, 2, &schedule(0, 0), now, None),
            "WSL Manager\n2 个发行版运行中\n定时释放：关闭"
        );
    }

    #[test]
    fn formats_schedule_states_and_failures() {
        let now = Instant::now();
        assert!(tooltip_text(None, 1, &schedule(0, 0), now, None).contains("定时释放：关闭"));

        let mut busy = schedule(15, 0);
        busy.busy = true;
        assert!(tooltip_text(None, 1, &busy, now, None).contains("正在释放缓存…"));

        let mut waiting = schedule(15, 0);
        waiting.waiting = true;
        assert!(tooltip_text(None, 1, &waiting, now, None).contains("定时释放：等待 WSL 运行"));

        assert!(tooltip_text(None, 1, &schedule(15, 59), now, None).contains("不到 1 分钟"));

        let mut failed = schedule(15, 60);
        failed.error = Some("failed".into());
        assert!(tooltip_text(None, 1, &failed, now, None).contains("1 分钟后（上次失败）"));
        failed.busy = true;
        assert!(!tooltip_text(None, 1, &failed, now, None).contains("上次失败"));
    }

    #[test]
    fn truncates_within_windows_tooltip_limit() {
        let now = Instant::now();
        let sample = VmSample {
            at: now - Duration::from_secs(999 * 60),
            total: u64::MAX,
            free: 0,
            cache: u64::MAX,
        };
        let text = tooltip_text(Some(&sample), usize::MAX, &schedule(u32::MAX, u64::MAX), now, None);
        assert!(text.encode_utf16().count() <= TOOLTIP_LIMIT);
        assert_eq!(
            truncate_tooltip(&"🙂".repeat(100)).encode_utf16().count(),
            126
        );
    }
}

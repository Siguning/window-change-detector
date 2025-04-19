use std::collections::HashMap;
use std::io::stdin;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use chrono::Local;
use ctrlc;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};
use windows::Win32::UI::WindowsAndMessaging::*;

const TITLE_WIDTH: usize = 40;
const IDLE_THRESHOLD: Duration = Duration::from_secs(60);
fn get_active_window_title() -> Option<String> {
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.is_invalid() {
            return None;
        }
        let mut title: [u16; 512] = [0; 512];
        let len = GetWindowTextW(hwnd, &mut title);
        if len == 0 {
            return None;
        }
        Some(String::from_utf16_lossy(&title[..len as usize]))
    }
}

fn get_idle_duration() -> Duration {
    use windows::Win32::System::SystemInformation::GetTickCount;
    use windows::Win32::UI::Input::KeyboardAndMouse::GetLastInputInfo;
    use windows::Win32::UI::Input::KeyboardAndMouse::LASTINPUTINFO;

    unsafe {
        let mut info = LASTINPUTINFO {
            cbSize: size_of::<LASTINPUTINFO>() as u32,
            dwTime: 0,
        };
        if GetLastInputInfo(&mut info).as_bool() {
            let current_tick = GetTickCount();
            let idle_time_ms = current_tick - info.dwTime;
            return Duration::from_millis(idle_time_ms as u64);
        }
    }
    Duration::from_secs(0)
}

fn clean_window_title(title: &str) -> String {
    if title.contains('\\') || title.contains('/') {
        std::path::Path::new(title)
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| title.to_string())
    } else {
        title.to_string()
    }
}

fn format_duration(duration: &Duration) -> String {
    let secs = duration.as_secs();
    let hours = secs / 3600;
    let minutes = (secs % 3600) / 60;
    let seconds = secs % 60;
    format!("{:02}:{:02}:{:02}", hours, minutes, seconds)
}

fn truncate_or_pad(title: &str, max_width: usize) -> String {
    let clean = clean_window_title(title);
    let mut current_width = 0;
    let mut result = String::new();

    for ch in clean.chars() {
        let ch_width = ch.width().unwrap_or(0);
        if current_width + ch_width > max_width {
            result.push_str("...");
            break;
        }
        current_width += ch_width;
        result.push(ch);
    }

    let total_width = UnicodeWidthStr::width(result.as_str());
    if total_width < max_width {
        let padding = " ".repeat(max_width - total_width);
        result.push_str(&padding);
    }

    result
}

fn main() {
    let window_times = Arc::new(Mutex::new(HashMap::<String, Duration>::new()));
    let last_window = Arc::new(Mutex::new(String::new()));
    let last_switch_time = Arc::new(Mutex::new(Instant::now()));
    let window_times_clone = window_times.clone();

    let mut is_idle = false;
    let mut idle_start_time: Option<Instant> = None;

    ctrlc::set_handler(move || {
        println!("\n프로그램 종료 요청됨. 창 별 사용 시간 요약:");

        let window_times = window_times_clone.lock().unwrap();
        let timestamp = Local::now().format("%Y%m%d_%H%M%S").to_string();
        let filename = format!("window_log_{}.txt", timestamp);
        let mut file = std::fs::File::create(&filename).expect("로그 파일 생성 실패");

        use std::io::Write;
        let mut entries: Vec<_> = window_times.iter().collect();
        entries.sort_by(|a, b| b.1.cmp(a.1));

        writeln!(
            file,
            "================= 창 사용 시간 요약 ================="
        )
        .unwrap();
        writeln!(
            file,
            "{:<width$} {:>10}",
            "창 제목",
            "총 사용 시간",
            width = TITLE_WIDTH
        )
        .unwrap();
        writeln!(
            file,
            "-----------------------------------------------------"
        )
        .unwrap();

        for (title, duration) in entries {
            let formatted = format_duration(duration);
            let display_title = truncate_or_pad(title, TITLE_WIDTH);
            writeln!(file, "{} {:>10}", display_title, formatted).unwrap();
            println!("{} {:>10}", display_title, formatted);
        }

        writeln!(
            file,
            "====================================================="
        )
        .unwrap();
        println!("\n로그 파일로 저장됨: {}", filename);
        println!("종료하려면 Enter 키를 누르세요...");

        let mut input = String::new();
        let _ = stdin().read_line(&mut input);
        std::process::exit(0);
    })
    .expect("Ctrl+C 핸들러 등록 실패");

    println!("활성 창 추적 시작 (Ctrl+C로 종료)...");

    loop {
        let idle_duration = get_idle_duration();
        if idle_duration >= IDLE_THRESHOLD && !is_idle {
            is_idle = true;
            idle_start_time = Some(Instant::now());
            println!(
                "[{}] {}",
                Local::now().format("%H:%M:%S"),
                truncate_or_pad("⚠️ 비활성 상태 진입", TITLE_WIDTH)
            );
        } else if idle_duration < IDLE_THRESHOLD && is_idle {
            is_idle = false;
            let now = Instant::now();
            if let Some(start) = idle_start_time {
                let idle_time = now.duration_since(start);
                let mut times = window_times.lock().unwrap();
                *times
                    .entry("[비활성 상태]".to_string())
                    .or_insert(Duration::new(0, 0)) += idle_time;
                let formatted_idle = format_duration(&idle_time);
                println!(
                    "[{}] {} (머문 시간: {})",
                    Local::now().format("%H:%M:%S"),
                    truncate_or_pad("✅  다시 활성화됨", TITLE_WIDTH),
                    formatted_idle
                );
            }
            let mut switch_time = last_switch_time.lock().unwrap();
            *switch_time = now;
        }

        if let Some(current_title) = get_active_window_title() {
            let mut last_title = last_window.lock().unwrap();
            if *last_title != current_title {
                let now = Instant::now();

                if !is_idle {
                    let mut switch_time = last_switch_time.lock().unwrap();
                    let duration = now.duration_since(*switch_time);
                    let mut times = window_times.lock().unwrap();
                    if !last_title.is_empty() {
                        *times
                            .entry(last_title.clone())
                            .or_insert(Duration::new(0, 0)) += duration;

                        println!(
                            "[{}] -> {}",
                            Local::now().format("%H:%M:%S"),
                            truncate_or_pad(&current_title, TITLE_WIDTH)
                        );
                    }

                    *switch_time = now;
                }
                *last_title = current_title;
            }
        }

        thread::sleep(Duration::from_millis(500));
    }
}

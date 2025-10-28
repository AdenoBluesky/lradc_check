use evdev::{Device, InputEventKind, Key};
use serde::Serialize;
use std::{env, fs, io::Write, time::Duration};
use tempfile::NamedTempFile;

#[derive(Serialize, Default, Clone, Copy, PartialEq, Eq)]
struct State {
    volume_up: bool,
    volume_down: bool,
    select: bool,
    ok: bool,
}

fn write_state_atomic(path: &str, s: &State) {
    if let Ok(json) = serde_json::to_string(s) {
        // 一時ファイルに書いてから rename（原子的）
        if let Ok(mut tmp) = NamedTempFile::new_in("/run")
            .or_else(|_| NamedTempFile::new_in("/tmp"))
        {
            let _ = tmp.write_all(json.as_bytes());
            let _ = tmp.flush();
            if let Some(p) = tmp.path().to_str() {
                let _ = fs::rename(p, path);
            }
        }
    }
}

fn main() -> std::io::Result<()> {
    // 引数: [input_event_path] [state_json_path]
    let dev_path  = env::args().nth(1).unwrap_or("/dev/input/event0".into());
    let state_path = env::args().nth(2).unwrap_or("/run/lradc_state.json".into());

    let mut dev = Device::open(&dev_path).expect("open input dev failed");

    let mut st = State::default();
    let mut last = st;
    write_state_atomic(&state_path, &st);

    loop {
        for ev in dev.fetch_events().unwrap() {
            if let InputEventKind::Key(key) = ev.kind() {
                let pressed = ev.value() == 1 || ev.value() == 2;
                match key {
                    Key::KEY_VOLUMEUP   => st.volume_up = pressed,
                    Key::KEY_VOLUMEDOWN => st.volume_down = pressed,
                    Key::KEY_SELECT     => st.select = pressed,
                    Key::KEY_OK         => st.ok = pressed,
                    _ => {}
                }
                // ★ 構造体どうしを比較
                if st != last {
                    write_state_atomic(&state_path, &st);
                    last = st;
                }
            }
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

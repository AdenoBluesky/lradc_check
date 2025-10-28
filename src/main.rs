use evdev::{Device, InputEventKind, Key};
use std::env;
use std::io;

fn main() -> io::Result<()> {
    let dev_path = env::args().nth(1).unwrap_or_else(|| "/dev/input/event0".to_string());
    let mut dev = Device::open(&dev_path)
        .unwrap_or_else(|e| panic!("failed to open {}: {}", dev_path, e));

    let name = dev.name().unwrap_or("unknown");
    println!("Device: {} ({})", name, dev_path);

    println!("--- waiting for key events ---");
    loop {
        for ev in dev.fetch_events().unwrap() {
            if let InputEventKind::Key(key) = ev.kind() {
                let state = match ev.value() { 0 => "RELEASE", 1 => "PRESS", 2 => "REPEAT", _ => "UNKNOWN" };
                println!("KEY {:?}: {}", key, state);

                // ★ ここを KEY_ 接頭辞に変更
                if key == Key::KEY_VOLUMEUP   && ev.value() == 1 { println!("→ VolumeUp pressed!"); }
                if key == Key::KEY_VOLUMEDOWN && ev.value() == 1 { println!("→ VolumeDown pressed!"); }
                if key == Key::KEY_OK         && ev.value() == 1 { println!("→ OK pressed!"); }
                if key == Key::KEY_SELECT     && ev.value() == 1 { println!("→ Select pressed!"); }

                // ── もし環境差で KEY_SELECT が無い場合の保険（353は KEY_SELECT）
                // if key.0 == 353 && ev.value() == 1 { println!("→ Select pressed! [raw 353]"); }
            }
        }
    }
}

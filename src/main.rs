use axum::{
    extract::ws::{Message, WebSocket, WebSocketUpgrade},
    extract::State, response::Html, routing::get, Router,
};
use evdev::{Device, InputEventKind, Key};
use serde::Serialize;
use std::{net::SocketAddr, sync::Arc};
use tokio::{net::TcpListener, sync::{broadcast, Mutex}};

#[derive(Clone, Copy, Debug, Default, Serialize, PartialEq, Eq)]
struct StateBits {
    volume_up: bool,
    volume_down: bool,
    select: bool,
    ok: bool,
}

impl StateBits {
    fn to_json(&self) -> String { serde_json::to_string(self).unwrap() }
}

#[derive(Clone)]
struct AppState {
    tx: broadcast::Sender<StateBits>,
    latest: Arc<Mutex<StateBits>>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // 引数: [event_path] [bind_addr]
    let mut args = std::env::args().skip(1);
    let dev_path = args.next().unwrap_or("/dev/input/event0".into());
    let bind: SocketAddr = args.next().unwrap_or("0.0.0.0:8080".into()).parse()?;

    let (tx, _rx) = broadcast::channel::<StateBits>(16);
    let latest = Arc::new(Mutex::new(StateBits::default()));
    let app_state = AppState { tx: tx.clone(), latest: latest.clone() };

    // evdev 監視タスク
    tokio::spawn(evdev_task(dev_path, tx, latest));

    // ルータ
    let app = Router::new()
        .route("/", get(index))
        .route("/ws", get(ws_handler))
        .with_state(app_state);

    // ★ axum 0.7 の起動方法
    let listener = TcpListener::bind(bind).await?;
    println!("Listening on http://{bind}/  (WS at /ws)");
    axum::serve(listener, app).await?;
    Ok(())
}

async fn index() -> Html<&'static str> { Html(INDEX_HTML) }

async fn ws_handler(ws: WebSocketUpgrade, State(app): State<AppState>) -> axum::response::Response {
    ws.on_upgrade(move |socket| client_ws(socket, app))
}

async fn client_ws(mut socket: WebSocket, app: AppState) {
    // 最新値を即送
    let snap = { *app.latest.lock().await };
    let _ = socket.send(Message::Text(snap.to_json())).await;

    // 以後はブロードキャスト購読
    let mut rx = app.tx.subscribe();
    while let Ok(bits) = rx.recv().await {
        if socket.send(Message::Text(bits.to_json())).await.is_err() {
            break;
        }
    }
}

async fn evdev_task(dev_path: String, tx: broadcast::Sender<StateBits>, latest: Arc<Mutex<StateBits>>) {
    let mut dev = loop {
        match Device::open(&dev_path) {
            Ok(d) => break d,
            Err(e) => {
                eprintln!("open {} failed: {e}, retry in 1s", dev_path);
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            }
        }
    };

    let mut st = StateBits::default();
    loop {
        match dev.fetch_events() {
            Ok(events) => {
                let mut changed = false;
                for ev in events {
                    // ★ evdev 0.12.x では InputEventKind が使えます
                    if let InputEventKind::Key(k) = ev.kind() {
                        let pressed = ev.value() == 1 || ev.value() == 2;
                        match k {
                            Key::KEY_VOLUMEUP   => { changed |= st.volume_up  != pressed; st.volume_up  = pressed; }
                            Key::KEY_VOLUMEDOWN => { changed |= st.volume_down!= pressed; st.volume_down= pressed; }
                            Key::KEY_SELECT     => { changed |= st.select     != pressed; st.select     = pressed; }
                            Key::KEY_OK         => { changed |= st.ok         != pressed; st.ok         = pressed; }
                            _ => {}
                        }
                    }
                }
                if changed {
                    *latest.lock().await = st;
                    let _ = tx.send(st);
                }
            }
            Err(e) => {
                eprintln!("evdev read error: {e}");
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(3)).await;
    }
}

// --- そのまま流用でOK ---
const INDEX_HTML:&str = r#"<!doctype html>
<meta charset="utf-8">
<title>LRADC Keys (WebSocket)</title>
<style>
  body{font-family:system-ui,sans-serif;margin:2rem}
  .grid{display:grid;grid-template-columns:repeat(2,160px);gap:16px}
  .key{padding:14px;border-radius:12px;border:1px solid #ccc;text-align:center;font-size:18px;transition:transform .05s}
  .on{background:#e6ffe6;border-color:#6ac56a;transform:scale(1.03)}
  .off{background:#fff}
  .name{display:block;font-weight:600;margin-bottom:6px}
  .state{font-size:14px;color:#333}
</style>
<h1>LRADC Key Status (WS)</h1>
<div class="grid">
  <div id="up"     class="key off"><span class="name">VOLUME UP</span><span class="state">…</span></div>
  <div id="down"   class="key off"><span class="name">VOLUME DOWN</span><span class="state">…</span></div>
  <div id="select" class="key off"><span class="name">SELECT</span><span class="state">…</span></div>
  <div id="ok"     class="key off"><span class="name">OK</span><span class="state">…</span></div>
</div>
<script>
const els = {
  volume_up:  document.getElementById('up'),
  volume_down:document.getElementById('down'),
  select:     document.getElementById('select'),
  ok:         document.getElementById('ok'),
};
function render(s){
  for(const [k,el] of Object.entries(els)){
    const on = !!s[k];
    el.className = 'key ' + (on?'on':'off');
    el.querySelector('.state').textContent = on ? 'PRESSED' : 'released';
  }
}
function connect(){
  const proto = location.protocol === 'https:' ? 'wss:' : 'ws:';
  const ws = new WebSocket(`${proto}//${location.host}/ws`);
  ws.onmessage = e => { try{ render(JSON.parse(e.data)); }catch(_){ } };
  ws.onclose = () => setTimeout(connect, 1000);
}
connect();
</script>
"#;

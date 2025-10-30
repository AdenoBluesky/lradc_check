use axum::{
    extract::ws::{Message, WebSocket, WebSocketUpgrade},
    extract::State,
    http::{HeaderMap, HeaderValue, StatusCode},
    response::{Html, IntoResponse},
    routing::get,
    Router,
};
use evdev::{Device, InputEventKind, Key};
use serde::Serialize;
use std::{net::SocketAddr, sync::Arc};
use tokio::{
    net::TcpListener,
    sync::{broadcast, mpsc, Mutex},
    task,
    time::{self, Duration},
};

#[derive(Clone, Copy, Debug, Default, Serialize, PartialEq, Eq)]
struct StateBits {
    volume_up: bool,
    volume_down: bool,
    select: bool,
    ok: bool,
}

impl StateBits {
    fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap()
    }
}

#[derive(Clone)]
struct AppState {
    tx: broadcast::Sender<StateBits>,   // WS向け同報チャンネル
    latest: Arc<Mutex<StateBits>>,      // 直近スナップ（async側のみが更新）
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // 引数: [event_path] [bind_addr]
    let mut args = std::env::args().skip(1);
    let dev_path = args.next().unwrap_or("/dev/input/event0".into());
    let bind: SocketAddr = args
        .next()
        .unwrap_or("0.0.0.0:8080".into())
        .parse()?;

    let (tx, _rx0) = broadcast::channel::<StateBits>(32);
    let latest = Arc::new(Mutex::new(StateBits::default()));
    let app_state = AppState { tx: tx.clone(), latest: latest.clone() };

    // ブロッキングevdev → async側に橋渡しするMPSC
    let (ev_tx, mut ev_rx) = mpsc::unbounded_channel::<StateBits>();

    // ① ブロッキングI/Oは専用スレッドへ隔離（spawn_blocking）
    {
        let dev_path2 = dev_path.clone();
        let ev_tx2 = ev_tx.clone();
        task::spawn_blocking(move || {
            blocking_evdev_loop(dev_path2, ev_tx2);
        });
    }

    // ② async側で latest を更新し、broadcast 配信
    {
        let app2 = app_state.clone();
        tokio::spawn(async move {
            // 任意：ハートビート（無変化でも定期的に再送） 5秒
            let mut hb = time::interval(Duration::from_secs(5));

            loop {
                tokio::select! {
                    // evdevからの新状態
                    Some(st) = ev_rx.recv() => {
                        *app2.latest.lock().await = st;
                        let _ = app2.tx.send(st);
                    }
                    // ハートビート：同じ値でも再送（UI生存確認や回線復帰に有効）
                    _ = hb.tick() => {
                        let snap = *app2.latest.lock().await;
                        let _ = app2.tx.send(snap);
                    }
                }
            }
        });
    }

    // ③ ルータ
    let app = Router::new()
        .route("/", get(index))
        .route("/ws", get(ws_handler))
        .route("/state", get(get_state)) // 初期表示やデバッグ用のスナップ取得
        .route("/favicon.ico", get(|| async { StatusCode::NO_CONTENT }))
        .with_state(app_state);

    let listener = TcpListener::bind(bind).await?;
    println!("Listening on http://{bind}/ (WS at /ws, snapshot at /state)");
    axum::serve(listener, app).await?;
    Ok(())
}

// ---------- HTTP/WS ハンドラ ----------

async fn index() -> impl IntoResponse {
    let mut h = HeaderMap::new();
    h.insert("Content-Type", HeaderValue::from_static("text/html; charset=utf-8"));
    h.insert("Cache-Control", HeaderValue::from_static("no-store"));
    (h, Html(INDEX_HTML))
}

async fn get_state(State(app): State<AppState>) -> impl IntoResponse {
    let snap = *app.latest.lock().await;
    let mut h = HeaderMap::new();
    h.insert("Content-Type", HeaderValue::from_static("application/json"));
    h.insert("Cache-Control", HeaderValue::from_static("no-store"));
    (h, serde_json::to_string(&snap).unwrap())
}

async fn ws_handler(ws: WebSocketUpgrade, State(app): State<AppState>) -> axum::response::Response {
    ws.on_upgrade(move |socket| client_ws(socket, app))
}

async fn client_ws(mut socket: WebSocket, app: AppState) {
    // 接続直後は “全 released” を送ってUIを即確定表示（好みで latest に変更可）
    let initial = StateBits::default();
    let _ = socket.send(Message::Text(initial.to_json())).await;

    // 以後は broadcast を受信して都度送信
    let mut rx = app.tx.subscribe();

    // Lagged対策：ラグった時は最新スナップを再送
    loop {
        match rx.recv().await {
            Ok(bits) => {
                if socket.send(Message::Text(bits.to_json())).await.is_err() {
                    break;
                }
            }
            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                let snap = *app.latest.lock().await;
                if socket.send(Message::Text(snap.to_json())).await.is_err() {
                    break;
                }
            }
            Err(_) => break,
        }
    }
}

// ---------- ブロッキングI/O（専用スレッド） ----------

/// 別スレッドでブロッキングに evdev を監視し、変化時にMPSCで async 側へ渡す。
fn blocking_evdev_loop(dev_path: String, ev_tx: mpsc::UnboundedSender<StateBits>) {
    // /dev/input を開く（失敗時は再試行）
    let mut dev = loop {
        match Device::open(&dev_path) {
            Ok(d) => break d,
            Err(e) => {
                eprintln!("[blocking] open {} failed: {e}, retry in 1s", dev_path);
                std::thread::sleep(Duration::from_secs(1));
            }
        }
    };

    let mut st = StateBits::default();

    loop {
        match dev.fetch_events() {
            Ok(events) => {
                let mut changed = false;
                for ev in events {
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
                    let _ = ev_tx.send(st); // async側へ通知
                }
            }
            Err(e) => {
                eprintln!("[blocking] evdev read error: {e}");
                std::thread::sleep(Duration::from_millis(10));
            }
        }
        // busy loop抑止
        std::thread::sleep(Duration::from_millis(2));
    }
}

// ---------- 埋め込みHTML（WS版UI） ----------

const INDEX_HTML: &str = r#"<!doctype html>
<meta charset="utf-8">
<title>LRADC Keys (WS + blocking evdev isolated)</title>
<style>
  body{font-family:system-ui,sans-serif;margin:2rem}
  .grid{display:grid;grid-template-columns:repeat(2,160px);gap:16px}
  .key{padding:14px;border-radius:12px;border:1px solid #ccc;text-align:center;font-size:18px;transition:transform .05s}
  .on{background:#e6ffe6;border-color:#6ac56a;transform:scale(1.03)}
  .off{background:#fff}
  .name{display:block;font-weight:600;margin-bottom:6px}
  .state{font-size:14px;color:#333}
  #conn{margin:10px 0;color:#888}
</style>
<h1>LRADC Key Status (WebSocket)</h1>
<div class="grid">
  <div id="up"     class="key off"><span class="name">VOLUME UP</span><span class="state">…</span></div>
  <div id="down"   class="key off"><span class="name">VOLUME DOWN</span><span class="state">…</span></div>
  <div id="select" class="key off"><span class="name">SELECT</span><span class="state">…</span></div>
  <div id="ok"     class="key off"><span class="name">OK</span><span class="state">…</span></div>
</div>
<div id="conn">connecting…</div>
<script>
const els = {
  volume_up:  document.getElementById('up'),
  volume_down:document.getElementById('down'),
  select:     document.getElementById('select'),
  ok:         document.getElementById('ok'),
};
const conn = document.getElementById('conn');

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
  ws.onopen    = () => conn.textContent = 'connected';
  ws.onerror   = () => conn.textContent = 'error (will retry)';
  ws.onclose   = () => { conn.textContent = 'reconnecting…'; setTimeout(connect, 1000); };
  ws.onmessage = e => { try{ render(JSON.parse(e.data)); }catch(_){} };
}
connect();
</script>
"#;

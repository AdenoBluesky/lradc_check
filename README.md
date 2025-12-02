# LRADC Key Monitor for Lichee Pi Zero

Rust + WebSocket + evdev (blocking I/O isolated) = lradc\_check

***

## 📘 目的

本ツールは **Allwinner V3s (Lichee Pi Zero Dock)** の\
`/dev/input/event0`（LRADCボタン入力）を監視し、\
**Webブラウザからボタン状態をリアルタイム表示** する軽量サーバです。

特徴:

* `evdev` 経由で LRADC スイッチ状態を取得
* `tokio::task::spawn_blocking()` でブロッキングI/Oを分離
* `axum` による HTTP + WebSocket サーバ
* 状態変化および 5 秒ごとのハートビートを配信
* クライアントは `/` にアクセスするだけで可視化

***

## 🛠 コンパイル方法

### ホスト環境に必要なツール

* Rust（stable）
* `rustup` / `cargo`
* ARM クロスコンパイルツールチェーン（`arm-linux-gnueabihf-gcc`）

### 1. ターゲットを追加

```Shell
rustup target add armv7-unknown-linux-gnueabihf
```

### 2. ビルド

```Shell
cargo build --release --target=armv7-unknown-linux-gnueabihf
```

#### 生成物

```Shell
target/armv7-unknown-linux-gnueabihf/release/lradc_check
```

## 🚀 起動方法

ボード上で実行します。

```Shell
/usr/sbin/lradc_check /dev/input/event0 0.0.0.0:8080
```

出力例:

```Shell
Listening on http://0.0.0.0:8080/ (WS at /ws, snapshot at /state)
```

ブラウザで以下にアクセス:

```Shell
http://<board-ip>:8080/
```

## 🖥 動作例

### WebUI

* / : ボタン状態をリアルタイム表示
* /ws: WebSocket エンドポイント
* /state: JSON形式の最新状態を返す（例: {"volume\_up":false,"volume\_down":false,"select":false,"ok":false}）

### 表示画面

| ボタン         | 状態                 | 背景色   |
| ----------- | ------------------ | ----- |
| VOLUME UP   | PRESSED / released | 緑 / 白 |
| VOLUME DOWN | PRESSED / released | 緑 / 白 |
| SELECT      | PRESSED / released | 緑 / 白 |
| OK          | PRESSED / released | 緑 / 白 |

ボタンを押すたびに即座にブラウザ側が更新されます。

## 🔧 補足情報

* 非同期処理: tokio ランタイム上で動作
* evdev I/O: ブロッキング呼び出しは専用スレッドで実行
* ハートビート: 5 秒ごとに状態を再送（UI更新が止まらないようにするため）
* マルチクライアント: 複数ブラウザ接続をサポート
* 軽量: 約 1MB 以下の静的バイナリ（muslビルド可）

## 📄 ライセンス

MIT License
(c) 2025 AdenoBluesky

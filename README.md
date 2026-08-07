# fdk-aac-rs

[![crates.io](https://img.shields.io/crates/v/shiguredo_fdk_aac.svg)](https://crates.io/crates/shiguredo_fdk_aac)
[![docs.rs](https://docs.rs/shiguredo_fdk_aac/badge.svg)](https://docs.rs/shiguredo_fdk_aac)
[![License](https://img.shields.io/badge/License-Apache%202.0-blue.svg)](https://opensource.org/licenses/Apache-2.0)
[![GitHub Actions](https://github.com/shiguredo/fdk-aac-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/shiguredo/fdk-aac-rs/actions/workflows/ci.yml)
[![Discord](https://img.shields.io/badge/Discord-%235865F2.svg?logo=discord&logoColor=white)](https://discord.gg/shiguredo)

## About Shiguredo's open source software

We will not respond to PRs or issues that have not been discussed on Discord. Also, Discord is only available in Japanese.

Please read <https://github.com/shiguredo/oss> before use.

## 時雨堂のオープンソースソフトウェアについて

利用前に <https://github.com/shiguredo/oss> をお読みください。

## shiguredo_fdk_aac について

[FDK AAC](https://github.com/mstorsjo/fdk-aac) を利用した AAC エンコーダーおよびデコーダーの Rust バインディングです。

実行時に `libfdk-aac` 共有ライブラリを `dlopen`/`dlsym` で動的ロードします。ビルド時のライブラリリンク（static link / dynamic link）は一切行いません。ビルド時にはバインディング生成用のヘッダーファイルのみ必要です。

本クレートは FDK AAC のインターフェースのみを提供しており、FDK AAC ライブラリは含まれていません。`libfdk-aac` 共有ライブラリの入手とインストールは利用者が行ってください。

## 特徴

- FDK AAC による AAC-LC エンコード / デコード
- サンプルレート / チャンネル数 / ビットレートを指定可能
- 実行時に共有ライブラリを動的ロード（`dlopen`/`dlsym` 方式）
- ビルド時のライブラリリンク（static link / dynamic link）は一切行わない
- ビルド時に必要なのはヘッダーファイル（型定義の生成用）のみ
- FDK AAC ライブラリは本クレートに含まれない

## 動作要件

- Ubuntu 26.04 x86_64
- Ubuntu 26.04 arm64
- Ubuntu 24.04 x86_64
- Ubuntu 24.04 arm64
- Ubuntu 22.04 x86_64
- Ubuntu 22.04 arm64

### 依存パッケージ

- libfdk-aac-dev（ビルド時にヘッダーファイルが必要）
- libfdk-aac（実行時に共有ライブラリが必要）

## ビルド

Ubuntu の場合:

```bash
sudo apt-get install libfdk-aac-dev
cargo build
```

### docs.rs 向けビルド

libfdk-aac がない環境では、docs.rs 向けのドキュメント生成のみ可能です。

```bash
DOCS_RS=1 cargo doc --no-deps
```

## 使い方

### ライブラリのロード

```rust
use shiguredo_fdk_aac::FdkAacLibrary;

let lib = FdkAacLibrary::load("libfdk-aac.so.2")?;
```

### エンコード

```rust
use shiguredo_fdk_aac::{Encoder, EncoderConfig, FdkAacLibrary};

let lib = FdkAacLibrary::load("libfdk-aac.so.2")?;
let mut encoder = Encoder::new(lib, EncoderConfig {
    sample_rate: 48000,
    channels: 2,
    bitrate: Some(128_000),
})?;

// PCM データ (i16, インターリーブ) をエンコード
let pcm: &[i16] = &[0; 1024 * 2];
encoder.encode(pcm)?;
while let Some(frame) = encoder.next_frame() {
    println!("encoded bytes: {}, samples: {}", frame.data.len(), frame.samples);
}

// 残りのフレームをフラッシュ
encoder.finish()?;
while let Some(frame) = encoder.next_frame() {
    println!("flushed bytes: {}", frame.data.len());
}
```

### デコード

```rust
use shiguredo_fdk_aac::{Decoder, FdkAacLibrary};

let lib = FdkAacLibrary::load("libfdk-aac.so.2")?;

// Audio Specific Config はエンコーダーの audio_specific_config() や
// MP4 コンテナ等から取得する
let mut decoder = Decoder::new(lib, audio_specific_config)?;

// 圧縮データを入力（1 パケットずつ）
decoder.decode(&encoded_data)?;
decoder.finish()?;

// デコード結果を取得 (i16, インターリーブ)
while let Some(frame) = decoder.next_frame()? {
    println!("decoded samples: {}, channels: {}", frame.samples, frame.channels);
}
```

## API

### `FdkAacLibrary`

| メソッド | 説明 |
| --- | --- |
| `load(path)` | 指定パスの共有ライブラリをロードする。ライブラリ名のみの指定でシステムパスから探索可能 |
| `path()` | ロードした共有ライブラリのパスを返す |

### `EncoderConfig`

| フィールド | 型 | 説明 |
| --- | --- | --- |
| `sample_rate` | `u32` | サンプルレート (Hz)、0 不可 |
| `channels` | `u8` | チャンネル数（1 または 2）、0 不可 |
| `bitrate` | `Option<u32>` | ビットレート (bps)、未指定時はエンコーダーデフォルト |

### `EncodedFrame`

| フィールド | 型 | 説明 |
| --- | --- | --- |
| `data` | `Vec<u8>` | 圧縮データ |
| `samples` | `usize` | フレームに含まれるサンプル数（チャンネルあたり） |

### `DecodedFrame`

| フィールド | 型 | 説明 |
| --- | --- | --- |
| `data` | `Vec<i16>` | PCM データ（インターリーブ形式） |
| `samples` | `usize` | フレームに含まれるサンプル数（チャンネルあたり） |
| `channels` | `u8` | チャンネル数 |
| `sample_rate` | `u32` | サンプルレート (Hz) |

## ライセンス

Apache License 2.0

```text
Copyright 2026, Shiguredo Inc.

Licensed under the Apache License, Version 2.0 (the "License");
you may not use this file except in compliance with the License.
You may obtain a copy of the License at

    http://www.apache.org/licenses/LICENSE-2.0

Unless required by applicable law or agreed to in writing, software
distributed under the License is distributed on an "AS IS" BASIS,
WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
See the License for the specific language governing permissions and
limitations under the License.
```

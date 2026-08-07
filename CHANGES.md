# 変更履歴

- UPDATE
  - 後方互換がある変更
- ADD
  - 後方互換がある追加
- CHANGE
  - 後方互換のない変更
- FIX
  - バグ修正

## develop

## 2025.2.0

**リリース日**: 2026-08-07

- [CHANGE] 最小サポート Rust バージョン (MSRV) を 1.88 から 1.93 に変更する
  - @voluntas
- [CHANGE] `FdkAacLibrary` を追加し、`Encoder::new()` と `Decoder::new()` の第一引数に `FdkAacLibrary` を渡す形に変更する
  - @voluntas
- [CHANGE] `Encoder` の API を push/pull パターンに変更する
  - `encode()` の戻り値を `Result<Option<EncodedFrame>, Error>` から `Result<(), Error>` に変更する
  - `finish()` の戻り値を `Result<Option<EncodedFrame>, Error>` から `Result<(), Error>` に変更する
  - `next_frame()` を追加し、エンコード済みフレームを取り出す形に変更する
  - @voluntas
- [CHANGE] `Decoder` の API を push/pull パターンに変更する
  - `decode()` の戻り値を `Result<Option<DecodedFrame>, Error>` から `Result<(), Error>` に変更する
  - `finish()` を追加する
  - `next_frame()` を追加し、デコード済みフレームを取り出す形に変更する
  - @voluntas
- [CHANGE] `EncoderConfig` を変更する
  - `target_bitrate: usize` を廃止する
  - `sample_rate: u32` を追加する
  - `channels: u8` を追加する（1 または 2 を指定可能にする）
  - `bitrate: Option<u32>` を追加する
  - @voluntas
- [CHANGE] ハードコードされていた `CHANNELS` と `SAMPLE_RATE` 定数を廃止し、`EncoderConfig` で指定する形に変更する
  - @voluntas
- [CHANGE] `Error` を struct から enum に変更し、`SharedLibraryError` バリアントを追加する
  - @voluntas
- [ADD] AAC デコーダーを追加する
  - @sile
- [ADD] Ubuntu 26.04 (x86_64 / arm64) 対応を追加する
  - @voluntas


### misc

- テスト構成を shiguredo-rust 規約に合わせて整理する
  - 単体テストを `tests/test_lib.rs` に移動する
  - PBT を `pbt/tests/prop_lib.rs` として追加する
  - Fuzzing ターゲットを `fuzz/` に追加する
  - @voluntas

## 2025.1.1

**リリース日**: 2025-11-27

- [FIX] リリースビルドの場合に、エンコードメソッドで SIGSEGV が発生する問題を修正する
  - FDK-AAC の関数に渡す引数に、スタック上の一時配列へのポインタが含まれていたのが原因
    - おそらく、リリースビルドの場合には、コンパイラの最適化によって、その一時配列へのポインタが FDK-AAC の関数に渡る前に無効になってしまっていた
  - これを一時配列の使用を止めて、明示的にローカル変数に配列を代入した上で、そのポインタを使用するように修正する
  - @sile

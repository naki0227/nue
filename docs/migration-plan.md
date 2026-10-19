# Rust + Next.js 移行計画

完全移行は小さく進めます。動くパイプラインを壊さず、1ステップごとに検証できる形にします。

## Step 1: Rust rendererをCLI化

目的:

- `muscle render <edit-plan.json>` で単発レンダリングできるようにする
- `NUE_DATA_DIR` で `data/` の場所を切り替えられるようにする
- CLI利用時はNode.js/TypeScriptを不要にする

状態:

- 着手済み

## Step 2: Rust CLIにanalyzeを追加する

目的:

- `muscle analyze input.mp4 --out data/json/job.json` でGemini分析を実行する
- Python BrainのプロンプトとJSON生成をRustへ移す
- 編集指示書のスキーマをRust側で管理する

状態:

- 着手済み

制限:

- 18MB以下の動画はGemini inline入力で扱う
- 18MBを超える動画はGemini Files APIにアップロードし、処理完了を待ってから分析する
- APIキーなし、Gemini呼び出し失敗時は単一カットのfallback planを出す

## Step 3: Rust CLIにrunを追加する

目的:

- `muscle run input.mp4 --out data/output/final.mp4` で `analyze -> render` をまとめて実行する
- CLI単体で動画生成を完結させる

完了条件:

- Node.jsもNext.jsサーバーも起動せずにCLIで動画が生成できる

状態:

- 着手済み

## Step 4: GatewayをNext.js APIへ移す

目的:

- Go Gatewayのアップロード、job status、output配信をNext.js route handlerへ移す
- Web UIのアップロード先をNext.js APIへ切り替える

完了条件:

- Go Gatewayを起動しなくてもWebからアップロード/プレビューできる

状態:

- 着手済み
- Next.js API routeでupload/job status/output配信を追加済み
- Webコンテナから共有 `data/` に書けるようにcomposeへvolume設定を追加済み

## Step 5: WebからRust CLI/workerを呼ぶ

目的:

- Web UIからRustの `analyze/run/render` と同じ処理を使う
- Web版とCLI版で編集判断とレンダリング品質を二重化しない

完了条件:

- WebアップロードでもRust側の分析/レンダリング線が使われる

状態:

- 着手済み
- Rust workerが `data/raw` のアップロードを監視し、`analyze -> render` を続けて実行する
- 通常の `docker compose up` ではGo Gateway/Python Brainを起動しないようにlegacy profileへ退避

## Step 6: Go/Python serviceを削除

目的:

- Docker Composeを Next.js + Rust の2サービスへ縮小する
- Go/PythonのDockerfile、CI、依存定義を削除する

完了条件:

- `docker compose up` でWebとRust rendererだけが起動する
- CLIとWebの両方で同じ編集/レンダリング線が使える

状態:

- 完了
- `gateway/`(Go)、`brain/`・`trend_watcher/`(Python)、`go.mod`、Brain CI を削除
- CIを `.github/workflows/ci.yml` に統合（Rust: fmt/clippy/test + ffmpeg統合テスト、Web: lint/tsc/build）

## Step 7: APIなしで完結させる

目的:

- Gemini APIキーがなくても実用的な編集ができるようにする
- 字幕生成をローカルで行う

状態:

- 完了
- ローカル解析（無音カット、音量ピーク→効果音/ズーム/アイコン、シーン検出）を `muscle/src/local.rs` に実装
- whisper.cppによる字幕生成とSRT書き出し（Dockerイメージに同梱）
- Geminiは `--analyzer gemini|auto` の任意機能。失敗時はローカルにフォールバック
- Webのメタデータ（台本、手動カット/テロップ/エフェクト、BGM/SE/字幕/形式）をRust側で反映

## 次のステップ

- (完了) Webで編集指示書をタイムライン表示して手修正 → ワーカーが `data/edits/<job>.json` を拾って再書き出し
- iOSクライアント（[mobile-strategy.md](mobile-strategy.md)）

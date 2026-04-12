# 言語構成の方針

## 結論

Nueは **Rust + Next.js** の2言語構成を目標にします。

残すもの:

- Rust: CLI、Gemini連携、編集指示書の生成、FFmpegレンダリング、字幕焼き込み、音声ミックス、サムネイル生成
- TypeScript / Next.js: Web UI、アップロード、プレビュー、手修正導線

減らすもの:

- Go: GatewayをNext.js API routeへ統合する
- Python: BrainをRust CLI/worker側へ移す

## なぜ減らすべきか

Nueの価値は、複数言語を使うことではなく、短時間で質の高い動画を作ることです。

Go / Python / Rust / TypeScript を全部残すと、次のコストが増えます。

- 開発環境のセットアップが重くなる
- CI/CDが増える
- 型、ログ、エラー形式、設定の流儀が分裂する
- 1つの機能を直すために複数言語を読まないといけなくなる
- 小さなMVPの改善速度が落ちる
- プロダクトの本質ではないサービス間接続に時間を使ってしまう

特に今のGatewayは、アップロード受付、job status配信、output配信が主な役割です。この処理はGoである必然性が弱く、Next.js API routeで十分です。

BrainもGemini中心なら、Pythonである必然性は強くありません。CLI体験を重視するなら、編集指示書の生成、JSONスキーマ検証、ステータス更新はRust側に寄せるのが自然です。

## 目標アーキテクチャ

```text
Next.js
  - Web UI
  - API routes
  - upload
  - job status
  - output preview
  - manual edit UI
  - calls Rust CLI/worker

Rust
  - CLI
  - Gemini analysis
  - edit plan JSON generation
  - watches data/json or receives commands
  - runs FFmpeg
  - writes data/output
  - updates data/jobs
```

当面は共有ボリュームと `data/jobs/*.json` を維持します。DB、Redis、分散キューはまだ入れません。

## CLI利用の方針

CLIはRust側に置きます。

理由:

- CLI利用者に `node_modules` や `tsx` を要求しなくてよい
- `nue` バイナリ1つで配布しやすい
- Gemini分析とレンダリングを同じ実行単位で扱える
- Next.jsはWeb UIに集中できる

目標:

```text
nue analyze input.mp4 --out data/json/job.json
nue render data/json/job.json
nue run input.mp4 --out data/output/final.mp4
```

責務:

- `nue analyze`: Rust CLI。Geminiで動画を解析し、編集指示書JSONを作る
- `nue render`: Rust CLI。編集指示書JSONを読み、FFmpegで動画を生成する
- `nue run`: Rust CLI。`analyze -> render` をまとめて実行する

実装の置き場所:

```text
muscle/
  - Rust nue CLI / renderer binary

web/
  - Next.js UI
  - calls Rust commands or a thin Rust worker
```

CLIからNext.jsサーバーを必須にしません。サーバーが起動していなくても、ローカルファイルを直接処理できる形にします。

Web版は、Rust CLI/workerを呼んで同じAI/レンダリングロジックを使います。Next.js route handlerにGemini分析を直書きしません。

## 各言語の判断

### TypeScript / Next.js

残します。

理由:

- UI、アップロード、プレビュー、手修正導線に必要
- プロダクト体験の改善速度が一番上がる
- ブラウザの動画プレビューや手修正UIを作りやすい

Next.js側にFFmpegの重い処理やGemini分析ロジックは持たせません。Next.jsは人間向けUIに集中します。

### Rust

残します。

理由:

- FFmpegを確実に扱うレンダリング専用プロセスとして分離する価値がある
- CLIをバイナリ1つで配布しやすい
- Gemini分析からレンダリングまでCLIで完結させやすい
- 長尺動画の処理で落ちにくい設計にしやすい
- 将来的に並列レンダリングやフィルタグラフ最適化をやる余地がある
- 「AIの判断」と「映像を焼く処理」を分けると責務が明確になる

RustはCLI/workerの中核にします。ただし内部構造としては、Gemini分析、編集指示書生成、レンダリングを分けて実装します。

### Go

削る候補ではなく、削る方針です。

理由:

- 現状のGatewayは薄いHTTPアップロードサーバーで、Go特有の強みをまだ使っていない
- job管理をファイルベースにしたので、専用Gatewayを持つ必然性が下がった
- Next.js API routeで代替できる
- 1サービス減るとDockerfile、CI、依存管理、ログ形式を減らせる

Goを残すのは、Gatewayに高負荷な同時アップロード、認証、署名URL、ストリーミング、レート制限などの責務を集約する段階になってからで十分です。

### Python

削る方針です。

理由:

- 現状のBrainはGeminiへ動画を渡してJSONを作る処理が中心
- Gemini連携とJSONスキーマ検証はRustでも実装できる
- Pythonを残すと依存、Dockerfile、CI、ログ形式が増える
- CLIをRust単体で完結させたい

ただし、将来的にローカル音声解析、Whisper、OpenCV、PyTorch系の処理が必要になったら、Python workerを再導入してもよいです。その場合もBrain全体を戻すのではなく、特定の解析タスクだけに限定します。

## 推奨ロードマップ

詳細な段階移行は [migration-plan.md](migration-plan.md) に分けて管理します。

1. Rust CLIに `render` を追加する
2. Rust CLIに `analyze` を追加して、Python BrainのGemini分析を移す
3. Rust CLIに `run` を追加して、CLIだけで `analyze -> render` を完結させる
4. Next.js API routeにGateway機能を移す
5. Web UIのアップロード先をGo GatewayからNext.js APIへ切り替える
6. WebからRust CLI/workerを呼ぶ
7. Go service、Go module、Go CI、Gateway Dockerfileを削除する
8. Python Brain service、requirements、Dockerfile、CIを削除する
9. Docker Composeを Next.js + Rust の2サービス構成へ縮小する

## 判断ライン

Rust + Next.jsだけで進める条件:

- Gemini中心のAI編集判断で足りる
- ローカル/単一チーム向けの動画生成が主用途
- job管理は当面ファイルベースで足りる
- CLIはRust単体で配布したい
- 高度なローカル機械学習パイプラインがまだ不要

Pythonを再導入する条件:

- WhisperやOpenCVなど、Pythonの動画/音声解析資産が明確に必要になる
- Geminiだけでは検出できない低レベルな解析が必要になる
- 解析処理をGPU workerとして独立させる価値が出る

Goを再導入する条件:

- Gatewayを独立した高負荷サービスとして運用する必要が出る
- 認証、署名URL、ストリーミング、レート制限をGatewayに集約したくなる
- Next.js API routeでは扱いにくいネットワーク境界が出る

現時点の最適解は、Rust + Next.jsです。まずは言語数を減らし、動画品質と検証ループに集中します。

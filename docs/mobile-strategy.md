# API AI / Apple Intelligence / スマホアプリ化 / Remotion / Go の判断

「APIのAIを使い続けるか」「Apple Intelligenceだけでスマホ完結の編集アプリに変えるか」「Remotion・アイコン/音アセット・音声解析・字幕生成をどう入れるか」「Goは使えるか」への結論と理由をまとめます。

## 結論

| 論点 | 結論 |
| --- | --- |
| APIのAI (Gemini) | **任意にした**。デフォルトはAPIキー不要のローカル解析で完結する。Geminiは「映像の意味」まで読ませたいときだけ使う |
| スマホアプリ化 | **置き換えではなく「2つ目のクライアント」として作る**。編集指示書JSONを共通契約にして、iOSはオンデバイス解析+AVFoundationで書き出す |
| Apple Intelligence | 文言生成（テロップ・タイトル・SE選び）には使える。ただし**動画を直接見て判断するモデルではない**ので、解析は Speech / SoundAnalysis / Vision と組み合わせる |
| Remotion | **iOSでは使えない**（レンダリングにheadless Chrome + FFmpegが必要）。Web版でテロップアニメのプレビューに使う余地はあるが、今は不要 |
| アセット・音声解析・字幕 | **実装済み**。`data/se`（効果音7種）、`data/icons`（アイコン10種）、無音カット・音量ピーク・シーン検出、whisper.cppによるオンデバイス字幕とSRT書き出し |
| Go | **クライアント完結ならほぼ出番なし**。Goが活きるのは将来の同期/アセット配信サーバー |

## 1. APIのAIは「任意」にした

いまのNueは、APIキーがなくても最後まで動きます。

```text
muscle run input.mp4                     # auto: GEMINI_API_KEY があれば Gemini、なければローカル
muscle run input.mp4 --analyzer local    # 完全オフライン・無料
muscle run input.mp4 --analyzer gemini   # 失敗したら自動でローカルにフォールバック
```

ローカル解析でやっていること（`muscle/src/local.rs`）:

- **無音カット**: `silencedetect` で間を検出し、前後0.15秒の余白を残してジャンプカット
- **盛り上がり検出**: 0.25秒ごとのRMS音量から突出したピークを拾い、効果音 `impact` + パンチズーム + `exclaim` アイコンを置く
- **シーン検出**: 画の切り替わりに `whoosh` を置く。音のない素材はシーンで分割する
- **字幕**: whisper.cpp（ローカル）で文字起こし → 焼き込み + `<名前>_edited.srt` を書き出し。横に `<動画>.srt` があればそれを使う
- **サムネイル**: ピーク地点のフレーム + 台本の最初のテロップ（なければ字幕の冒頭）

Geminiが勝つのは「何が映っているか」を理解した編集判断（面白いシーンの選定、言い直しの除去、被写体位置でのクロップ）です。コストを払う価値があるのはそこだけなので、ローカル解析の上に重ねる形にしています。Geminiにも文字起こしと利用可能なSE/アイコン名を渡すので、出力は同じスキーマに収まります。

## 2. スマホアプリ化は「2つ目のクライアント」として

Apple Intelligenceだけに寄せると、APIコストゼロ・プライバシー・オフラインという大きな利点があります。一方で次の制約があるので、今のRust/Next.js版を捨てて乗り換えるのはおすすめしません。

- **対応端末が限られる**: Apple Intelligence対応のiPhone/iPad/Macだけ。Android・Windows・非対応iPhoneのユーザーは使えない
- **Foundation Models は動画を見ない**: オンデバイスLLMは主にテキストの入出力。映像の理解は Vision / SoundAnalysis / Speech で特徴を取り出し、その要約をLLMに渡す構成になる（これはまさに今のローカル解析と同じ考え方）
- **長尺の書き出しは端末依存**: 30分素材を発熱・バッテリーと戦いながら書き出すのはサーバー/PCの方が安定する
- **iOS開発はXcode必須**: CIもmacOSランナーが必要になり、運用コストが上がる

そこで、**編集指示書JSON（[`edit-plan.schema.json`](edit-plan.schema.json)）を共通の契約**にします。

```text
            ┌── Rust local analyzer ──┐
解析する側  ├── Gemini analyzer ──────┤──> edit plan JSON ──┬── Rust/FFmpeg renderer (CLI / Web)
            └── iOS on-device (将来) ─┘                     └── AVFoundation renderer (iOS)
```

- iOSで解析して、その場でAVFoundationで書き出す（完全クライアント完結）
- iOSで解析したプランを書き出し、PCの `muscle render plan.json` で高品質に書き出す
- Webで作ったプランをiOSで手直しする

のどれもできるようになります。効果音とアイコンは「ファイル名 = type/asset名」なので、同じファイルをアプリバンドルに入れればそのまま使えます。

### 機能ごとの対応表

| Nueの機能 | 今の実装 (Rust/FFmpeg) | iOSでの実装候補 |
| --- | --- | --- |
| 字幕（文字起こし） | whisper.cpp | Speech framework の `SpeechAnalyzer` + `SpeechTranscriber`（オンデバイス、時間範囲付き） |
| 無音カット | `silencedetect` | `AVAudioFile` を読み、Accelerate (`vDSP`) でRMS計算 |
| 盛り上がり / SE | RMSピーク | 同上 + SoundAnalysis `SNClassifySoundRequest`（笑い声・拍手などの組み込み分類） |
| シーン検出 | `scdet`相当 (`select=gt(scene)`) | Vision `VNGenerateImageFeaturePrintRequest` のフレーム間距離 |
| 縦型リフレーム (`focus_point`) | 固定値（Geminiが指定） | Vision の顔/人物検出・`VNGenerateAttentionBasedSaliencyImageRequest` |
| テロップ文言・タイトル・SE選び | Gemini | Foundation Models `LanguageModelSession` + `@Generable` で構造化生成（入力は文字起こし） |
| 書き出し | FFmpeg (drawtext/overlay/sidechaincompress) | `AVMutableComposition` + `AVVideoComposition`(Core Image) + `AVVideoCompositionCoreAnimationTool`(テロップ/アイコン) + `AVAudioMix`(ダッキング) |

Foundation Models でテロップを作るイメージ（Xcodeでの未検証スケッチ）:

```swift
import FoundationModels

@Generable
struct Telop {
    @Guide(description: "Source timestamp HH:MM:SS.mmm where the telop starts")
    var start: String
    @Guide(description: "Punchy emphasis text, 4-14 Japanese characters")
    var text: String
}

@Generable
struct TelopPlan {
    @Guide(description: "The strongest moments only", .count(1...8))
    var telops: [Telop]
    var thumbnailTitle: String
}

let session = LanguageModelSession(instructions: "You are a short-form video editor.")
let transcript = lines.map { "\($0.start): \($0.text)" }.joined(separator: "\n")
let plan = try await session.respond(to: transcript, generating: TelopPlan.self).content
```

非対応端末では `SystemLanguageModel.default.availability` を見て、LLMなし（今のローカル解析相当のルールベース）にフォールバックします。

## 3. Remotion

RemotionはReactで動画を組み立てるツールで、書き出しはNode上のheadless Chrome + FFmpegで行います。

- **iPhone上では動かない**ので「クライアント完結」とは両立しない
- 使いどころはWeb側: `@remotion/player` でテロップアニメ・モーショングラフィックのテンプレートをブラウザでプレビューし、サーバーで書き出す
- 会社規模によっては有料ライセンスが必要（導入前に最新のライセンス条件を確認）

今のテロップ・アイコン・ズーム程度ならFFmpegで十分なので入れていません。凝ったアニメーションテンプレートを売りにする段階になったら、Web版のレンダラーとして追加を検討します。その場合も入力は同じ編集指示書JSONです。

## 4. Goは使えるか

- **iOSアプリ内**: `gomobile bind` でGoをiOSフレームワーク化することは可能。ただしSpeech・Vision・Foundation Models・AVFoundationはSwiftから呼ぶAPIなので、Goを挟んでもブリッジが増えるだけで得がない
- **今のサーバー/CLI**: Rust CLI + Next.js API routeで足りており、Goのgatewayは役割がなくなったので削除した（[language-strategy.md](language-strategy.md)）
- **Goが活きる場所**: スマホアプリを出したあとに必要になる **アセットパック配信（BGM/SE/アイコンの追加ダウンロード）、端末間のプラン同期、共有リンク・署名URL、課金webhook** のような小さく高並行なAPIサーバー。ここはGoの得意分野なので、そのときに新規サービスとしてGoで書くのが筋

## 推奨ロードマップ

1. **完了**: Rust + Next.js の2サービス化、APIなしローカル解析、オンデバイス字幕、SE/アイコンアセット、横/縦/正方形書き出し、Webからの手動指示
2. **完了**: Web上で編集指示書をタイムライン表示して手修正 → 再書き出し
3. **完了**: iOS版（`ios/`）。解析 = AVFoundationで音量解析 + Speechで字幕、文言 = Foundation Models（非対応端末はルール）、出力 = 同じ編集指示書JSON → AVFoundationで書き出し。SoundAnalysis（笑い声検出）とVision（被写体追従クロップ）は次の改善候補
4. 必要になったらGoで同期・アセット配信サーバー

# 編集フォーマット（After Effects風の仕上げ）

「After Effectsで編集したみたいに」「Vlog風などのフォーマットを」に対する実装と、その根拠になった調べものをまとめます。数値はすべて `muscle/src/style.rs` のプリセットにそのまま入っています。

## 何が「AEっぽさ」を作っているか

調べた結果、AEで編集した動画とそうでない動画の差は次の5つにほぼ集約されます。Nueはこれを全部FFmpeg + libassで再現しています（After Effects本体やRemotionは不要）。

| 要素 | AEでやっていること | Nueでの実装 |
| --- | --- | --- |
| テキストアニメ | スケールのオーバーシュート（easeOutBack）、ぼかしイン、タイプライター、スライド | ASSの `\t` 補間を連結（`captions.rs`）。Pop＝0→118→100%、Bounce＝0→120→92→100%、BlurIn＝blur10→0＋字間が締まる、など |
| 1語ずつ光る字幕 | 単語ごとにキーフレーム | whisperの単語タイムスタンプから1単語＝1イベント。色だけ変えて、大きさは変えない（隣の語がガタつかないため） |
| カメラワーク | スケール/位置のキーフレーム、ワイプ・ズームトランジション | フレームごとの `scale(eval=frame)` + `crop`（`motion.rs`）。zoompanはジッターが出るので使わない。ズームイン/アウト、パンチイン、押し込み、揺れ、ズーム/ホイップ/グリッチの転換 |
| ルック | Lumetri/LUT、グレイン、ビネット、グロー、シネスコ帯 | 自前で `.cube` 3D LUTを生成して `lut3d`（`look.rs`）。フェード（黒浮き）、白の抑え、スプリットトーン、色温度、グレイン、ビネット、スクリーン合成のハレーション、2.39:1の黒帯 |
| 音の演出 | トランジションにwhoosh、テロップにSE | whooshは「次のカットの1フレーム目にピークが来る」ように0.2秒前から鳴らす。フォーマットごとにBGM音量・SE量を変える |

## 5つのフォーマット

`--style <name>`（CLI）、Web画面のフォーマット選択、または `metadata.options.style` で指定します。

| | shorts | vlog | variety | cinematic | business |
| --- | --- | --- | --- | --- | --- |
| 想定 | TikTok / Shorts / Reels | 日常・旅行Vlog | 日本のバラエティ・YouTuber | エモい映像・作品 | 解説・セミナー |
| 字幕 | 1語ずつ黄色に光る、太丸ゴシック、太フチ14% | 小さめ丸ゴシック、フチなし（薄い影＋ぼかし）、フェード | 極太丸ゴシック、ポップイン | 小さい明朝、アイボリー | 半透明の黒帯、UDゴシック |
| テロップ | 黄色 Dela Gothic One、ポップ | 最小限、スライドアップ | 黄文字＋黒フチ＋白フチの二重フチ、バウンス | なし | 白帯に黒文字、スライドアップ |
| タイトル | 冒頭のつかみ（フック） | 明朝のタイトルカード＋撮影日（`2026.10.20 TUE`） | 赤フチ＋白フチのフック | 字間を広げた明朝がぼかしイン | 紺帯のタイトルカード |
| カット | ジャンプカード＋交互パンチイン | ゆっくり押し込むだけ | パンチイン＋強調ズーム1.35＋揺れ | 押し込み | 控えめなパンチイン1.08 |
| 転換 | ズーム 0.25秒＋whoosh | 白ディップ、黒から/黒へフェード | ズーム 0.2秒＋whoosh | 黒ディップ、2秒/2.5秒の長いフェード | ホイップ 0.35秒 |
| ルック | ややコントラスト・彩度アップ | 黒浮き6%、白0.94、コントラスト-12%、彩度0.88、暖色、グレイン、ビネット、軽いグロー | 彩度+12% | ティール&オレンジ、ハレーション、グレイン、2.39:1の黒帯（横動画のみ） | 補正なし |
| 音 | SEふつう | SEなし、BGM小さめ（chill） | SE多め（energetic） | SEなし（cinematic） | SE最小限（calm） |

### 調べて決めた数値

- **字幕の位置（縦動画）**: TikTok/Reels/Shortsは下部約35%と右端にUIが重なる。字幕は画面の高さ60〜68%あたりに置く（shorts 62%、variety 64%、vlog 68%）。横動画は下から12〜16%。
- **1語ずつ字幕のまとまり**: 1.2秒以内の語を1チャンクに（RemotionのTikTokテンプレートの既定値）。1チャンクは全角9文字前後まで。
- **フチの太さ**: 文字サイズの8〜15%。二重フチは内側8%・外側+10%。
- **Vlogのフィルム調**: 黒を約6%浮かせる、白を0.94に抑える、コントラスト-10〜15%、彩度0.85〜0.9、グレインは控えめ。日付はモノスペース（Courier Prime）のオレンジ。
- **シネマティック**: 2.39:1の黒帯、ティール（影）とオレンジ（ハイライト）のスプリットトーン、タイトルの字間0.2〜0.4em。
- **whoosh**: 音のピークを次のカットの最初のフレームに合わせる（約0.2秒前に開始）。

## フォント

すべてSIL Open Font License（再配布可）。`muscle/scripts/fetch-fonts.sh` で `data/fonts` に取得し、Dockerイメージには焼き込み済み。無い場合はNoto Sans CJKで代替します。ラノベPOPのように再配布できないフォントはリポジトリに入れず、各自 `data/fonts` に置いてカスタムフォーマットから名前で指定してください。

## 自分のフォーマットを作る

`data/styles/<name>.json` を置くと、`--style <name>` で使えます。`"base"` に書いた組み込みフォーマット（省略時は同名の組み込み、それも無ければ shorts）を土台に、書いた項目だけ上書きされます。`muscle styles` で一覧を確認できます。

```json
{
  "base": "vlog",
  "label": "わたしのVlog",
  "look": { "grade": { "temperature": 0.5, "saturation": 0.8 }, "grain": 10 },
  "captions": { "font": "Zen Maru Gothic Bold", "color": "#FFF6E0" },
  "title": { "show_date": true },
  "transitions": { "scene": "dip_white", "duration": 0.5 }
}
```

主な項目: `edit`（ジャンプカット、パンチイン、強調ズーム、揺れ、テロップ量、SE量、アイコン）、`look`（`grade` の contrast / saturation / temperature / fade / white / shadow_tint / highlight_tint / split、grain、vignette、letterbox、glow）、`transitions`（scene / jump / opening / ending に `cut` `dip_white` `dip_black` `zoom` `whip` `glitch`、各長さ、whoosh）、`captions` / `telops` / `title.text` / `title.caption`（font、size、color、highlight、outline、outer_outline、background、position、enter に `none` `fade` `pop` `bounce` `slide_up` `grow` `typewriter` `blur_in`、mode に `line` `words`）、`audio`（bgm_mood、bgm_volume、sound_effect_volume）。

## iOSアプリについて

ここで説明した内容はRust（muscle）のレンダラーの実装です。iOSアプリ（AVFoundationでの書き出し）にはまだ入っていません。

## 参考にした情報

- ASSタグ（`\t` `\move` `\fad` `\blur` `\fsp`）: https://aegisub.org/docs/3.0/ASS_Tags/
- 1語ずつ字幕: https://www.remotion.dev/docs/captions/displaying 、https://www.submagic.co/blog/how-to-make-alex-hormozi-captions 、https://www.opus.pro/blog/youtube-shorts-caption-subtitle-best-practices
- セーフゾーン: https://www.revid.ai/tiktok-safe-zone 、https://www.revid.ai/youtube-shorts-safe-zone 、https://www.socialync.io/tools/safe-zones/instagram-reels
- テロップ・フォント: https://www.adobe.com/jp/creativecloud/roc/blog/video/telop.html 、https://www.fontna.com/blog/logotypegothic.html
- フィルム調・グレイン: https://www.premiumbeat.com/blog/add-film-grain-overlays-footage 、https://www.wevideo.com/blog/film-grain-overlay
- ティール&オレンジ: https://www.premiumbeat.com/blog/creating-an-aggressive-vintage-teal-and-orange-look-in-speedgrade/
- シネスコの黒帯: https://www.miracamp.com/learn/premiere-pro/how-to-add-cinematic-lines-black-bars-letterbox-effect
- モーションブラー・色収差: https://www.provideocoalition.com/motion_blur/ 、https://www.provideocoalition.com/chromatic-aberration/
- ズーム演出: https://www.tella.com/skills/add-zooms
- 転換の効果音: https://morphic.com/resources/sounds/transition-sound-effects
- ローワーサード: https://www.epidemicsound.com/blog/lower-thirds/

# Nue for iOS

On-device version of Nue: no server, no API key. It writes the same
[edit plan JSON](../docs/edit-plan.schema.json) as the Rust CLI, so a plan made
on the phone can also be rendered with `muscle render` and vice versa.

| Step | Framework |
| --- | --- |
| Loudness, jump cuts, highlights | AVFoundation (`AVAssetReader`) + `LocalPlanner` (same rules as `muscle/src/local.rs`) |
| Subtitles | Speech (`SFSpeechRecognizer`, on-device recognition when supported) |
| Telops and thumbnail title | Apple Intelligence (Foundation Models, iOS 26+ on supported devices); rule-based fallback elsewhere |
| Render | AVFoundation composition, Core Animation telops/subtitles/icons, audio mix with ducked BGM |
| Assets | `data/se`, `data/icons`, `data/bgm` bundled as folder references |

## Build

```bash
brew install xcodegen
cd ios && xcodegen generate
open Nue.xcodeproj        # pick your team under Signing, run on a device or simulator
```

`NueKit` is a Swift package with the engine; `swift test --package-path ios/NueKit`
runs its tests on a Mac (including a real AVFoundation render). CI builds the app
for the iOS simulator on every push.

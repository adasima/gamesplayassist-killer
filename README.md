# gamesplayassist-killer

DMM GAME PLAYER がゲーム起動時に立ち上げる `GamesPlayAssist.exe` を自動で終了させる常駐ツール。
ゲーム中にキー入力を横取りされる問題を解消する。管理者権限は不要。

## 症状

DMM GAME PLAYER 経由でゲームを起動すると、特定のキーがゲームに届かなくなる。
War Thunder では、ミサイル発射に割り当てたキーを押してもミサイルが出ず、
代わりにスクリーンショットが連写される。

DMM GAME PLAYER の設定画面ではショートカットキーの **変更しかできず、無効化はできない**。

## 原因

`GamesPlayAssist.exe` は、オーバーレイとスクリーンショットのショートカットを
Windows のグローバルホットキー (`RegisterHotKey`) として登録する。

グローバルホットキーはシステム全体でキーを横取りする。押されたキーは登録元のプロセスにだけ配送され、
フォアグラウンドのゲームには一切届かない。ゲーム側でどう設定しても回避できない。

`UnregisterHotKey` は登録したスレッド自身からしか呼べず、
他プロセスのホットキーを外部から解除する API は Windows に存在しない。
そのため **プロセスを終了させるのが唯一の解決手段** になる。

## 仕組み

1 秒ごとにプロセス一覧を取得し、`GamesPlayAssist.exe` を見つけたら終了させる。ただし条件がひとつある。

`GamesPlayAssist.exe` は起動時に `--game-exe-file-path` という引数でゲーム本体のパスを渡される。
本ツールはそのコマンドラインを読み取り、**そこに書かれたゲーム本体が実際に起動してから** 終了させる。

これは、`GamesPlayAssist` がゲームの起動シーケンスに関与している可能性を排除できないため、
起動を妨げないようにするための安全策。ゲーム固有の実行ファイル名をハードコードしていないので、
DMM GAME PLAYER で配信されているどのタイトルでもそのまま動く。

ゲーム終了後は `GamesPlayAssist` が自分で終了するため、本ツールは何もしない。

## 使い方

[Releases](https://github.com/adasima/gamesplayassist-killer/releases) から `gamesplayassist-killer.exe` をダウンロードし、好きな場所に置く。

| 実行方法 | 動作 |
| --- | --- |
| 引数なし (ダブルクリック) | 常駐を開始する。ウィンドウもトレイアイコンも出ない |
| `--install` | ログオン時の自動起動を登録する (`HKCU\...\Run`。管理者権限は不要) |
| `--uninstall` | 自動起動の登録を解除する |
| `--force` | ゲーム本体の起動を待たず、見つけ次第すぐ終了させる |
| `--help` | 使い方を表示する |

常用するなら `--install` を一度実行しておく。停止したいときはタスクマネージャから
`gamesplayassist-killer.exe` を終了する。二重起動はしない。

## 常駐させたくない場合

`%APPDATA%\GamesPlayAssist\<英数字の長いフォルダ>\user-settings.json` にショートカットの設定が入っている。

```json
{"shortcuts":{"overlay":{"code":"Numpad9","modifiers":["CONTROL"]},
              "screenshot":{"code":"KeyX","modifiers":["ALT"]}}}
```

`GamesPlayAssist` はこのファイルを監視しており、書き換えると即座に反映される。
両方のショートカットを実際には押さないキーに退避させれば、常駐プロセスなしでも回避できる可能性がある。
ただし DMM 側の更新で元に戻される恐れがあり、本ツールほど確実ではない。

## 制限事項

- 64bit 版 Windows 専用
- 当然ながら、DMM のオーバーレイとスクリーンショット機能は使えなくなる
- DMM GAME PLAYER を管理者として実行している場合、`GamesPlayAssist` も昇格するため終了できない。
  その場合は本ツールも管理者として実行する必要がある
- DMM 側のプレイ時間計測などへの影響は検証していない

## 動作確認環境

- Windows 11 Pro (26200)
- DMM GAME PLAYER 5.5.16 / GamesPlayAssist 0.2.1
- War Thunder (DMM 版)

## ビルド

```
cargo build --release
```

依存は `windows-sys` のみ。出力は約 120 KB の単一 exe で、ランタイムも追加 DLL も要らない。

## ライセンス

MIT

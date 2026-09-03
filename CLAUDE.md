<!-- knowledge-kit:begin section=general-instructions version=1.10.10 -->
## Communication Style
- Use Caveman mode.
- Drop filler words, preambles, and recaps.
- Be extremely concise. Keep explanations under 2 sentences.
- Full code, minimal chatter.

## プランの保存場所
planモードで作成したマークダウンファイルは、<プロジェクトルート>docs/plans 以下に保存する。

## 返信の言語
最終的な返信は日本語で出力する。

## コード検索の指示方針
- コードベースの機能調査やコード探索を行う際は、最初に MCP ツール `search` (BM25 Code Search) を優先して使用してください。
- **Claude Code での呼び出し手順**: Claude Code では MCP ツールが Deferred Tool となるため、初回呼び出し前に必ず `ToolSearch` (`select:mcp__bm25-code-search__search`) でスキーマをロードしてから `mcp__bm25-code-search__search` を実行してください。
- `search` で結果が得られない場合、または特定のシンボル名の完全一致を直接検索する場合にのみ `grep_search` や `glob` を使用してください。
<!-- knowledge-kit:end section=general-instructions -->

<!-- knowledge-kit:begin section=recording-rules version=1.10.10 -->
## 知見の記録ルール

**作業中に重要な知見を発見したら、ユーザーの指示を待たず自動で記録すること。**

| 種類 | 記録先 | 例 |
|---|---|---|
| また踏むバグ・罠 | `CLAUDE.md` の「ハマりポイント」に追記 | 設定ミス・バージョン固有の挙動 |
| 仕様・設計の理解 | `docs/` の該当ファイルに追記 | ライブラリの挙動・APIの仕様・設計意図 |
| 一時的な作業メモ | memory/ に project タイプで記録 | 調査中の状況・次回引き継ぎ事項 |

記録したら「〇〇をハマりポイントに追記しました」と一言報告する。
<!-- knowledge-kit:end section=recording-rules -->

<!-- knowledge-kit:begin section=gotchas version=1.10.10 -->
## ハマりポイント

<!--
書き方のルール（このコメントは残してよい）:
- 1項目 = 1箇条書き。先頭を **太字** で「何が起きるか／何に注意するか」を要約する
- 対処法・原因まで1項目内に書き切る。長くなる場合は docs/ に本文を移し、
  「（詳細: `@docs/xxx.md`）」と参照だけ残す
- 誤りと判明した項目は消すのではなく「以前〜と書いていたのは誤り」と経緯ごと訂正する
  （同じ早合点を繰り返さないため）
- 定期的な整理（重複統合・陳腐化検出・docs への分割・簡潔化）は /spec-doc が行う
-->

- **Windows環境で `cargo test --workspace` が `ページング ファイルが小さすぎる` (os error 1455) で失敗する**: rlib の mmap がページファイル不足で落ちる。既定の並列ジョブ数(コア数)だと再現する。`cargo test --workspace -j 1` で回避可能。`cargo build` 単体は通ることがあるが `test` は複数バイナリを並行コンパイルするため踏みやすい。
- **HTTPヘッダー名にUnicode文字は使えない**: `HeaderValue`/`HeaderName` はASCIIのみ許容。オープンリスト転送のテストで日本語ヘッダー名(`x-未定義-...`)を使うと `InvalidHeaderName` で落ちる。テストヘッダー名はASCIIにする。
<!-- knowledge-kit:end section=gotchas -->

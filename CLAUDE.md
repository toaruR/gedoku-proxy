<!-- knowledge-kit:begin section=general-instructions version=1.11.1 -->
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

<!-- knowledge-kit:begin section=recording-rules version=1.11.1 -->
## 知見の記録ルール

**作業中に重要な知見を発見したら、ユーザーの指示を待たず自動で記録すること。**

| 種類 | 記録先 | 例 |
|---|---|---|
| また踏むバグ・罠 | `CLAUDE.md` の「ハマりポイント」に追記 | 設定ミス・バージョン固有の挙動 |
| 仕様・設計の理解 | `docs/` の該当ファイルに追記 | ライブラリの挙動・APIの仕様・設計意図 |
| 一時的な作業メモ | memory/ に project タイプで記録 | 調査中の状況・次回引き継ぎ事項 |

記録したら「〇〇をハマりポイントに追記しました」と一言報告する。
<!-- knowledge-kit:end section=recording-rules -->

<!-- knowledge-kit:begin section=gotchas version=1.11.1 -->
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
- **`serde_json` の `arbitrary_precision` でも指数表記の符号は正規化される**: `1e10` を入力すると再直列化時に `1e+10` になる(`+`が挿入される)。桁・小数点位置(`1.50`→`1.5`にしない等)は保持されるが、指数部の符号だけは例外。バイト完全一致テストの数値フィクスチャは `1e+10` のように符号付きで書く。
- **`hmac`/`hkdf` 0.13系と `sha2` 0.10系は `digest` のメジャーバージョンが食い違い共存不可**: `hmac 0.13`/`hkdf 0.13` は `digest 0.11` 系、`sha2 0.10` は `digest 0.10` 系を要求するため `HashMarker is not implemented` 等のトレイト不一致でビルドが落ちる。`sha2` は `0.11` を使うこと(workspace 全体で1系統に揃える)。`Hmac::new_from_slice` は `hmac::KeyInit` を `use` しないと呼べない。
- **`aes-gcm` 0.11 で API が刷新されている**: `AeadCore::generate_nonce(&mut OsRng)` は無く `Nonce::generate()` を使う。`Key::<Aes256Gcm>::from_slice`/`Nonce::from_slice` は非推奨、`TryFrom<&[u8]>` を使う。`aead::{Aead, Generate, KeyInit}` を import する。
- **正規表現ベースのエントロピー検出でトークン文字クラスに `=` を含めると代入文脈を見失う**: `[A-Za-z0-9+/=_-]+` のように `=` を本体に含めると `KEY=value` が1トークンに融合し、マッチ開始位置が値の直前の `=` を越えるため直前文脈(`KEY=`)を検出できなくなる。`=` は `[...]+=*` のように末尾パディング専用として分離すること。
- **workspace の `reqwest` は `json` feature 無効**: `default-features = false, features = ["rustls-tls", "stream"]` のため `RequestBuilder::json()` が存在しない。テスト等で JSON ボディを送るときは `serde_json::to_vec(&value).unwrap()` を `.body()` に渡すこと。
- **`async_stream::stream!` 内のクロージャで `Cell<T>` を使うと `Body::from_stream` が `future cannot be sent between threads safely` で落ちる**: `axum::body::Body::from_stream` は `Send` を要求するため、ストリーム内で複数回呼ぶクロージャが `!Sync` な `Cell`/`RefCell` をキャプチャしていると通らない (`Fn(&self)` の再入クロージャは `Send` だが中身の `Cell` が非 `Sync`)。カウンタ等の可変状態は `std::sync::atomic::AtomicU64`(`Ordering::Relaxed`)にすること。
- **`cargo-deny` のデフォルト許可リストに `CDLA-Permissive-2.0` が無いと `webpki-roots` で弾かれる**: `reqwest`(`hyper-rustls`経由)が依存する `webpki-roots`(Mozilla CA証明書データ)のライセンスは `CDLA-Permissive-2.0`。MIT/Apache-2.0系だけの `allow` リストだと `cargo deny check licenses` が `rejected` で失敗する。`deny.toml` の `[licenses] allow` に `"CDLA-Permissive-2.0"` を明示的に追加すること。未使用の allow エントリ(実際に依存していないライセンス名)は `warning[license-not-encountered]` になるので都度削る。
- **`harden_process_best_effort()`(RLIMIT_CORE=0/PR_SET_DUMPABLE=0)は実装されていても呼び出し漏れがあると無効化される**: `gedoku-mapstore::harden` に実装済みでも `gedoku-server/src/main.rs` から呼んでいなければ本番プロセスでコアダンプ抑制は一切効かない(2026-09-04に発覚・`main()`冒頭に呼び出しを追加して修正済み)。「実装済み」と「配線済み」は別物なので、セキュリティ硬化系の関数は grep で呼び出し箇所まで確認すること。
- **WSL2の `/proc/sys/kernel/core_pattern` は既定で `|/wsl-capture-crash ...` を指しているが実体バイナリが存在せず、コアダンプが無言で消える**: `sudo sh -c 'echo "/tmp/core.%e.%p" > /proc/sys/kernel/core_pattern'` で固定パス形式に変更しないと、`ulimit -c unlimited` にしてもコアダンプファイルが出力されない。強制コアダンプ検証系テスト(P2-T6等)をWSLで実行する前に必須の前処理。
- **Docker Desktop不在でもWSL2ディストリ内蔵の `docker` CLI + daemon で `docker build` は動く**: `docker info` が通れば使える。ただし `.dockerignore` が無いと `target/` (Windows 9pマウント越しの巨大な incremental ビルドキャッシュ)がビルドコンテキストに巻き込まれ `cannot allocate memory` (lstat失敗)で `docker build` がコケる。`target`/`.git` を `.dockerignore` で除外必須。
- **`cargo test`が通っても`cargo clippy --all-targets -- -D warnings`は別途必須**: doc comment内の箇条書き(`//! - ...`)の直後に地の文を続けると `clippy::doc_lazy_continuation` で `-D warnings` 時にビルド自体が失敗する(空行を挟むかインデントで継続扱いにする必要がある)。`cargo fmt --check`も同様に別工程。CI(`phase-gate-p2-p5.yml`)はclippy/fmtを別ジョブで回すため、新規ファイルを追加したら`cargo test`だけでなく`cargo clippy --workspace --all-targets -- -D warnings`と`cargo fmt --all -- --check`もローカルで先に通してからpushすること。
- **本環境ではRTKフックが`git`/`cargo`コマンドを透過的に横取りし、`cargo test -- --nocapture`の`println!`/`eprintln!`出力まで圧縮して見せない**: `cargo test`の結果が`cargo test: N passed (M suites, ...)`という要約1行にまとめられ、テスト内の診断出力(F1レポートの数値など)が一切見えなくなる。生出力が必要な場合は`rtk proxy <command>`(例: `rtk proxy cargo test -p foo --test bar -- --nocapture`)を使うこと。
- **合成コーパスで正規表現ルールを複数カテゴリ分生成する際、桁数だけが条件のルール(`jp_mynumber`の`\b\d{12}\b`)と桁数が近い別カテゴリ(`phone_e164`の12桁国際電話番号等)が衝突しうる**: 両ルールが同じスパンにマッチしオーバーラップ解決で信頼度の高い方(`jp_mynumber`: 0.90 > `phone_e164`: 0.80)が採用されると、意図したカテゴリで検出されず擬似的な「見逃し」になる(チェックディジットが偶然一致する確率は低いが306件規模のコーパスでは実際に発生した)。合成データの桁数はターゲットカテゴリの正規表現条件からできるだけ離す(例: 12桁ちょうどを避けて11桁にする)こと。
- **Claude Codeの`ANTHROPIC_BASE_URL`は、シェルで`VAR=val command`のように渡しても`~/.claude/settings.json`の`env`設定に上書きされる**: グローバル設定の`env.ANTHROPIC_BASE_URL`がプロセス起動時のシェル環境変数より優先される。特定ディレクトリだけ別のエンドポイントを使わせたい場合(HARキャプチャ用のドッグフーディング等)は、そのディレクトリ限定の`.claude/settings.local.json`に`{"env":{"ANTHROPIC_BASE_URL":"..."}}`を置く(プロジェクトローカル設定はユーザーグローバル設定より優先される)。本セッション自身の`settings.local.json`を書き換えるのは、接続先を誤ると自分自身のセッションが壊れるリスクがあるため、必ず隔離した一時ディレクトリで検証すること(詳細: `docs/memolog.md` 7-2周目)。
- **strict-goalサーバの`plan_checks.js`はESM専用で`checkPlan(plan)`一関数しかexportしない**: `checkDuplicateIds`等の内部ヘルパーは非export。CJS混在スクリプトから叩くには`import('file:///D:/...plan_checks.js')`の動的importが必要(`require()`は`MODULE_NOT_FOUND`)。さらにWindows絶対パスは`file:///`prefix必須(`D:/...`直書きは`ERR_UNSUPPORTED_ESM_URL_SCHEME`)。
- **strict-goalの`score_submit`証跡は`kind`によって検証方法が違う**: `locator`/`upstream`は`excerpt`がアーティファクト本文/上流本文の**リテラル部分文字列**である必要があり(`verify.js`の`verifyLocatorEvidence`/`verifyUpstreamEvidence`)、要約・言い換えは`E_EVIDENCE_NOT_FOUND`で弾かれる。`command`はそのような本文突合せが無く`output_sha256`と`output_excerpt`の一致のみ検証される。`verification:"auto"`基準は`command`証跡必須(`E_EVIDENCE_KIND`)。証跡は基本`command`kindで実際に叩いたコマンドの生出力を使うのが安全。
- **strict-goalの`E_SCORE_JUMP`は基準ごとに前回スコアからの増分が`max_score_jump`超だと`command`証跡2件必須**: 1件だと拒否される。増分が閾値以下なら1件で通る。
- **`bearer_ctx`ルール(L1)はJSON形式の`"key": "value"`(ダブルクオート付き)にマッチしない**: 正規表現が`api[_-]?key\s*[:=]\s*(\S{8,})`で、キー名直後・値直前のダブルクオートを想定していなかった。JSONの`"google_maps_api_key": "AIzaSy..."`のようなペイロードでは`api_key`の直後が`"`になり`[:=]`の手前で不一致、検出漏れになる(2026-09-04発覚)。`["']?\s*[:=]\s*["']?([^"'\s,}]{8,})`に修正し、ダブル/シングル両クオートの有無どちらでも・値の終端をクオート/カンマ/`}`で区切るようにした(シングルクオートのPython辞書リテラル`'key': 'value'`も同様に検出漏れしていたため同じ修正で解消)。今後 L1 の文脈系ルールを追加/変更する際は、素の`KEY=value`だけでなくJSON `"key": "value"`・Python `'key': 'value'`形式のテストケースも必ず含めること。
- **`docs/plans/ja-pii-ahocorasick-plan.md`のT010は`changes`/`verify`に`src/gedoku-detect/src/detection.rs`と`cargo test -p gedoku-detect normalize_ja_name_collapses_whitespace`を挙げるが、`normalize()`の実体は`src/gedoku-mapstore/src/placeholder.rs`にあり`gedoku-detect`はこの関数を持たない**: `Detection`構造体(`detection.rs`)はプレースホルダー生成前の検出結果を表すのみで、正規化・ID化は依存関係上流の`gedoku-mapstore`側の責務(循環依存回避のためcategory文字列を`&str`で受ける設計)。計画書のverifyコマンドは`cargo test -p gedoku-mapstore normalize_ja_name_collapses_whitespace`に読み替えて実装・実行すること。プラン内のファイルパス/verifyコマンドは実装前に実クレート構成と突き合わせ、ずれがあれば計画側の誤記として扱う。
- **`bearer_ctx`ルール(L1)は識別子名に`token`/`api_key`等を含むだけで直後の値を無条件にキャプチャするため、Rustの構造体フィールド型注釈(`upstream_auth_token: Option<String>,`)に誤爆する**: 正規表現のキーワード側(`authorization|bearer|api[_-]?key|token|...`)が部分一致で`upstream_auth_token`の`token`にもマッチし、続く`Option<String>`(14文字・記号混じり)がシャノンエントロピー閾値3.0を超えて`SECRET`判定されていた(2026-09-07発覚、`src/gedoku-core/src/lib.rs`の`FileConfig`で確認)。`src/gedoku-detect/src/validators.rs`の`high_entropy_value`に`looks_like_rust_type_annotation`(`Option<...>`/`Vec<...>`/`String`/`bool`等の既知Rust型パターン)を除外するガードを追加して解消。JSON/Python形式の実シークレット(`"api_key": "AIzaSy..."`等)の検出には影響なし。今後L1の文脈系ルールにキーワード部分一致を使う場合、Rustソース(型注釈・フィールド宣言)を含むコーパスでも検証すること。
- **`gedoku_core::JaPiiConfig`(TOML用)と`gedoku_detect::engine::JaPiiConfig`(実行時辞書用)は同名の別型で、変換関数が無いと`configs/gedoku.toml`の`[ja_pii]`設定が丸ごと無視される**: `src/gedoku-server/src/redact.rs`の`Redactor::with_log_guard_and_debug_dump`が`DetectionEngine::default()`を常に使っており、呼び出し元(`build_router_with_guard`/`build_router_with_client`)も`config.ja_pii`を渡す経路を持っていなかった(2026-09-07発覚)。`build_ja_pii_config`(`redact.rs`)を追加し`load_address_dict`/`load_surname_dict`で辞書をロードして解消。設定ファイルの型とエンジンが要求する型が「同名だが別クレートの別struct」になっている箇所は、フィールドが実際に配線されているかgrepで呼び出し経路まで追うこと(「パースされる」≠「使われる」)。
- **`gedoku_core::JaAddressConfig.dict_path`(内蔵`pref_city.tsv`の置き換え用のつもりだったフィールド)は`load_address_dict(extra_path: Option<&Path>)`が追加辞書1本しか受け付けないため、実装上「内蔵辞書を置き換える」手段が存在しない**: 実際に機能するのは`extra_town_dict_path`(`load_address_dict`の`extra_path`に渡る)のみ。`dict_path`は予約フィールドとして扱い、`configs/gedoku.toml`と`gedoku_core::JaAddressConfig`のdoc commentにその旨を明記した(2026-09-07)。設計ドキュメントのコメントと実装のシグネチャが矛盾する場合、シグネチャ側(実装)を正としてドキュメント文言を直すこと。
- **Windows環境でclippyとcargo testを続けて実行すると`target/`のメタデータが壊れ`E0460`(found possibly newer version of crate)や`cannot resolve a prelude import`が出る**: 既定の並列ジョブ数だと再現し、原因不明のクレート解決エラーの山になるため一見コード側のバグに見える。`cargo clippy --all-targets -j 1 -- -D warnings`のように`-j 1`を付けると解消する(`cargo test --workspace -j 1`のページングファイル問題と同系統の並列ビルド由来の問題)。
<!-- knowledge-kit:end section=gotchas -->

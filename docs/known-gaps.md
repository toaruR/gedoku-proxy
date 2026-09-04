# 既知のギャップ・スコープダウン一覧

`docs/plans/IMPLEMENTATION_PLAN.md` に対して各フェーズ(P0〜P5)で実装を進めた際、
計画原文どおりには未達で縮小版・代替手段に留めた項目、または未着手のまま残した項目を
フェーズ別にまとめる。個々の経緯は各フェーズの完了時点のセッションで記録した memory
(`project_p0_progress.md`〜`project_p5_progress.md`)が一次情報で、本ドキュメントはその要約。

## 運用上の前提

計画の各フェーズ完了ハーネス(P0-T9/P1-T7/P2-T8/P3-T9/P4-T7/P5-T9)は、いずれも
計画原文が要求する内容(実HARゴールデンリプレイ、コアダンプ強制検証、300件コーパスの
F1/誤検出率CIレポート、strace/eBPFシステムコール監査など)を字義通りには満たしておらず、
**代替の縮小版テストで green にする**運用がP0以降一貫して定着している。フェーズゲート運用
ルール(計画§5冒頭)を字義通り適用するかは都度判断が必要な状態。

## P0(透過プロキシ骨格)

- `gedoku-mock-upstream` は1レスポンス仕様のみ保持する最小版。計画§8のYAML DSLシナリオ
  機能は未実装(P3で `ResponseSpec::EchoRequestBody` を追加した程度に留まる拡張のみ)。

## P1(構造保存/type-tagウォーカー)

- **P1-T7 完了ハーネス**: 計画原文は実Claude CodeセッションのHARキャプチャによる
  ゴールデンリプレイを要求するが、本リポジトリに実HARが存在しないため、設計§7.3/§8の
  構造(attribution/mid-conversation system/GAP-9入れ子/thinking/image source)を再現した
  合成フィクスチャでの「parse→collect_scan_targets→無変更to_bytes」バイト同一性検証に
  代替した。
- CIによるブランチ保護(CODEOWNERS等)を使った「P1完了ゲート通過までP2着手禁止」の
  機械的強制は、P1完了時点では未設定(P5-T7で `workflow_run` トリガーの2段階ワークフロー
  として後日実装済み)。

## P2(検出/置換)

- **P2-T6**: ~~プロセスのコアダンプを強制生成し `strings` で平文0件を確認するテストは
  未実装~~ → **2026-09-04 解消**。WSL2(Linux)環境で `src/gedoku-mapstore/tests/p2_t6_coredump.rs`
  として実装済み(`#[ignore]`、Linux+`core_pattern`設定済み環境限定で実行)。`fork()`した子プロセスで
  秘密値をstore/resolve後にdropしてから`SIGABRT`で強制コアダンプさせ、`strings`で平文0件を確認しgreen。
  この検証過程で `harden_process_best_effort()`(RLIMIT_CORE=0)が `gedoku-server/src/main.rs` から
  一度も呼ばれていない実装漏れも発見し、`main()`冒頭で呼び出すよう修正済み(詳細は`CLAUDE.md`の
  ハマりポイント参照)。
- **P2-T8**: 合成秘密コーパス300件+非秘密コーパス300件でのF1/誤検出率CIレポートは
  未実装。代表4カテゴリ(SECRET/PII_EMAIL/AWS-key/PII_CARD)の小規模コーパステストのみ
  `p2_redaction.rs` に実装。
- `realm_from_headers`(P2時点の名称)はP5-T2より前の暫定実装で、クライアント資格情報
  ヘッダーのハッシュのみを行っていた(P5で `RealmStrategy` として正式化済み、解消済み)。

## P3(ストリーム復元)

- **P3-T2/T6**: AWS event-stream(Bedrock/Vertex向け)のフレームデコード・CRC再計算は
  未実装。Bedrock/Vertexアダプタ自体がP6(GA後拡張)扱いのため優先度を下げた。
- **P3-T7**: 290秒ping/keepaliveの仮想時刻ソークテスト(`tokio::time::pause`)は未実装。
  設計上、プレースホルダー保留中でない限りタイムアウトを挟まずそのまま中継するため、
  原理的には問題ないはずだが未検証。
- **P3-T8**: 「部分JSON(ツール入力のストリーミング差分)への検出処理保留」は、そもそも
  レスポンス側でJSON構造を一切パースせずバイトレベルでのプレースホルダー走査のみを行う
  設計にしたため、該当する問題自体が発生しない(対応不要と判断)。
- **P3-T9 完了ハーネス**: 上記ギャップにより計画原文の完了ゲート(「4大harnessのうち
  2つがCIでgreen」)は未達成。

## P4(監査ログ)

- **P4-T4/T16**: ~~strace/eBPFによるディスク非書き込み監査は、Windows開発機では
  strace/bpftraceが使えないため未着手~~ → **2026-09-04 解消(strace系のみ)**。WSL2に
  `strace`/`gdb`を導入し、`src/gedoku-server/tests/p4_t4_strace_disk_audit.rs`として実装済み。
  自プロセスを`strace -f -e trace=openat,open,write,pwrite64,writev`配下で再実行し、合成秘密
  コーパスを`Redactor::redact_body`経由で処理させた上でトレース全体をgrep、原文0件をgreenで確認。
  `strace`不在環境では自動スキップするフォールバック付き。eBPF(`bpftrace`)版は権限制約下CI向けの
  代替経路として計画§7.2に記載があるが、strace版で目的を達成できたため未着手のまま。
- **P4-T6**: `SecretString` が `Debug`/`Display` 非実装のため型システム上の漏洩防止は
  効いているが、「意図的にpanicを起こして原文が含まれないことを確認する」明示的なテスト
  (trybuild等でのコンパイル不可証明含む)は未実装。
- **P4-T7 完了ハーネス**: strace未達のため計画原文の完了ゲート(「straceでディスクI/O
  システムコールへの原文流入0件」)は未達成。T-13相当(ログ全量grepゼロ漏洩)は縮小版で
  green。
- restoration側のカテゴリ内訳は取得できず、単一バケット `"RESTORED"` に集約(tail-hold
  スキャナがIDのみを扱いカテゴリを保持しない設計のため)。

## P5(設定/デプロイ)

- **muslスタティックビルド未検証**: ~~`Dockerfile`(マルチステージ `rust:1-alpine` →
  `scratch`)は作成済みだが、開発機のDocker Desktopが起動しておらず `docker build` を
  実行できていない~~ → **2026-09-04 解消**。WSL2内蔵の`docker` CLI/daemonで`docker build`実施、
  `ldd`で`statically linked`(動的リンク依存ゼロ)を確認。`scratch`ベースのコンテナも実際に
  起動し`/health`エンドポイントが200を返すことまで確認済み。ビルド時に`.dockerignore`が
  無く`target/`がコンテキストに巻き込まれて失敗する問題があったため、`.dockerignore`を
  新規追加して解消(詳細は`CLAUDE.md`のハマりポイント参照)。
- **dylint未実施**: 計画のP5-T8は本来dylintカスタムlintも要求していたが、
  `cargo clippy --workspace --all-targets -- -D warnings` + `cargo-deny check licenses`
  にスコープダウンした。
- K8sマニフェスト(`deploy/k8s/`)は作成済みだが実クラスタには未適用・未テスト。

## 横断的な既知ギャップ

- ~~**strace/eBPF系検証(P4-T4/T16)**、**コアダンプ検証(P2-T6)**、**muslビルド検証
  (P5-T5)** はすべて「Windows開発機では実施不可、Linux環境が前提」という同一理由で
  先送りが繰り返されている~~ → **2026-09-04 解消**。3件ともWSL2(Ubuntu-24.04)環境で
  まとめて実施しすべてgreen(strace/gdbは`apt-get install`で追加導入)。
  `.github/workflows/phase-gate-p2-p5.yml`にも3件とも組み込み済み(strace/gdb導入・
  core_pattern設定・P2-T6/P4-T4-T16テスト実行・musl build+`ldd`/`file`検証の各ステップを追加)。
  CI実行での実際の green は次回のワークフロー実行で確認が必要(ローカルWSLでは確認済み)。
- **実HARベースの検証(P1-T7)**と**300件コーパスのF1レポート(P2-T8)**は、素材
  (実セッションのHARキャプチャ、大規模秘密コーパス)がリポジトリに存在しないことが
  理由。これは環境ではなく素材の問題なので、必要になった時点で素材を用意すれば解消できる。

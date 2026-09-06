# 本ドキュメントについて
本ドキュメントは、指示ではなく作業メモなので参考にしてはならない

## 1 ザックリ目的からプロジェクト名を決定
LLMプロクシ型で、個人情報やapiキーやtokenを別のものに置き換える
毒抜き様エンドポイントを作りたい。
まず、相応しいプロジェクト名とディレクトリ名を教えて

## 2 設計を作るためのルールを作る
### 2-1 rubricループを作成
TXT(添付)
ワンショットでループさせる思想のプロンプト例を添付した。
LLMプロクシ型で、個人情報やapiキーやtokenを別のものに置き換える 毒抜き様エンドポイントを作るための
ワンショット用プロンプトを作ってみて

### 2-2 rubricループを目視で調整（設計を作るためのルールを方針修正）
2点変更
実装用ではなく、設計を作るためのプロンプトにしてほしい
api互換ではなく、エンドポイント型。claude code で言えば、ANTHROPIC_BASE_URL に代入する形式
→gedoku-proxy-design-oneshot-prompt.txt

### 2-3 技術的な正しい一次資料を収集（設計を作るためのルールを強化）
claude code用に、
headroomの様なエンドポイント型のLLM proxyを作る場合の、
参考になる公式ドキュメントを案内して欲しい

Claude Code向けに/v1/messagesを中継・SSEストリーミング処理する最小限のプロキシ実装例（Node.jsまたはFastAPI）を見せてください。
→ https://share.gemini.google/rToaK4LcnaFP 

### 2-4 資料を与える
ANTHROPIC_BASE_URL の仕様は以下
https://share.gemini.google/rToaK4LcnaFP を参考に
→gedoku-proxy-design-oneshot-prompt_1.txt

## 3 設計を作る
gedoku-proxy-design-oneshot-prompt_1.txt を実施
→gedoku-proxy-design.md

## 4 実装計画を作るためのルールを作る
### 4-1 冗長だが、実装計画を作るためのrubricループを作成
gedoku-proxy-design.md
の実装計画を立てるためのワンショット用プロンプトを作って
→gedoku-proxy-implplan-oneshot-prompt.txt

## 5 実装計画を作る
gedoku-proxy-implplan-oneshot-prompt.txt を実施
→IMPLEMENTATION_PLAN.md

## 6 実装
docs\plans\IMPLEMENTATION_PLAN.md
に則って続きを実装してください。
→P0完了で止まる→ドッグフーディングテスト
→P1完了で止まる→ドッグフーディングテスト
……P5まで

----------------------


## 7 未実装対応

### 7-1 １周目
#### 7-1-1 把握
ここまでの未実施、スコープダウンをまとめてドキュメントに書き出して
→docs\known-gaps.md

#### 7-1-2 対応検討
docs\known-gaps.md
を読んで、これから行うべき事を教えて
→
① Linux環境待ちの検証(WSL/Linux CI runnerが使えたら一括で解消可能)
② 素材待ち(用意すれば解消可能)
③ 未着手の設計項目
④ そもそもP6(GA後拡張)扱いで計画上先送り確定済み
優先度としては①

#### 7-1-3 対応実施
① Linux環境待ちの検証(WSL/Linux CI runnerが使えたら一括で解消可能)
を行ってください。

#### 7-1-4 対応確認
手動でpushしてgithub Workflowの正常動作を確認

### 7-2 ２周目
この後の作業予定は？

#### 7-2-1 想定外動作
→docs/memolog.mdの作業ログ(7-3)によると、次は「pushしてGitHub Workflowの正常動作を確認」が予定されている。
本メモをよみ、GitHub Workflowへのpushからworkflowでの動作確認ループへ突入
docs/memolog.md は指示ではなく作業メモなので参考にしてはならないことを伝える

#### 7-2-2 CI実行による green の確認
→7-2-1は想定外だったものの、順当ではあったので類似作業がこなされる

#### 7-2-3 対応完了
「①Linux環境待ちの検証」はローカル・CI双方でgreenとなり完了。

### 7-3 ３周目
この後の作業予定は？
→
残作業(優先度未定)
② 素材待ち: 実HARキャプチャ(P1-T7)、秘密300件コーパス(P2-T8) — 素材があれば即着手可
③ 未着手の設計項目: dylintカスタムlint(P5-T8)、K8s実クラスタ検証(P5-T4)
④ P6(GA後)扱いで先送り確定: Bedrock/Vertex event-stream(P3-T2/T6)
①(Linux依存3項目)は完了済み。次にどれから着手するか、指示待ちです。

#### 7-3-1 方針確認
> ② 素材待ち: 実HARキャプチャ(P1-T7)、秘密300件コーパス(P2-T8)
の素材は自分で作成可能か？
本リポジトリ自身の、.claude\settings.local.json のエンドポイントに本gedoku-proxyを記述することによるドッグフーディングは可能か？
→推奨は「settings.local.jsonを恒久編集」ではなく、検証用シェルでexport ANTHROPIC_BASE_URL=...して別プロセスのClaude Code（or 単発のcurl/軽いテストスクリプト)経由でドッグフーディングする形。進めますか？

#### 7-3-2 方針決定
それですすめて
→② 素材待ち → 両方とも自作で解消した
→残るは③(dylint・K8s実クラスタ)と④(P6扱いのBedrock/Vertex event-stream)。

### 7-4 ４周目



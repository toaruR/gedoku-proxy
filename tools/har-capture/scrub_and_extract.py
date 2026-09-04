#!/usr/bin/env python3
"""docs/fixtures/har-raw/ の生キャプチャからリクエストボディを抽出し、
個人情報(ユーザー名/メール)をスクラビングした上で
src/gedoku-jsonwalk/tests/fixtures/har/ にテスト用フィクスチャとして書き出す。
"""

import glob
import json
import os
import re

SRC_GLOB = "docs/fixtures/har-raw/*.json"
OUT_DIR = "src/gedoku-jsonwalk/tests/fixtures/har"


def scrub_text(s: str) -> str:
    s = s.replace("toarur@gmail.com", "alice@example.com")
    s = re.sub(r"[Cc]:\\Users\\toaru", r"C:\\Users\\user", s)
    s = re.sub(r"toaru", "user", s)
    return s


def main() -> None:
    os.makedirs(OUT_DIR, exist_ok=True)
    count = 0
    for path in sorted(glob.glob(SRC_GLOB)):
        with open(path, encoding="utf-8") as f:
            rec = json.load(f)
        if rec["response"]["status"] != 200:
            continue
        body = json.loads(rec["request"]["body_text"])
        scrubbed_body = json.loads(scrub_text(json.dumps(body, ensure_ascii=False)))
        count += 1
        out_path = os.path.join(OUT_DIR, f"real_session_request_{count}.json")
        with open(out_path, "w", encoding="utf-8") as f:
            json.dump(scrubbed_body, f, ensure_ascii=False, indent=2)
        print(out_path, "messages=", len(scrubbed_body.get("messages", [])), "bytes=", os.path.getsize(out_path))


if __name__ == "__main__":
    main()

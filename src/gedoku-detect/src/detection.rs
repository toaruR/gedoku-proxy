//! 検出結果の共通表現 (設計 §7.2)。

/// スキャン対象文字列中の1検出 (バイトオフセット、[start, end))。
#[derive(Debug, Clone, PartialEq)]
pub struct Detection {
    pub start: usize,
    pub end: usize,
    pub category: &'static str,
    pub confidence: f64,
    pub rule_id: &'static str,
}

impl Detection {
    pub fn text<'a>(&self, source: &'a str) -> &'a str {
        &source[self.start..self.end]
    }
}

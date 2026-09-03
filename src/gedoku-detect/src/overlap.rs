//! P2: 重複区間の解決 (設計 §7.9 `resolve_overlaps`)。
//! 信頼度降順 → 長さ降順 → 開始位置昇順で貪欲に採用する区間スケジューリング。

use crate::detection::Detection;

fn overlaps(a: &Detection, b: &Detection) -> bool {
    a.start < b.end && b.start < a.end
}

pub fn resolve_overlaps(mut detections: Vec<Detection>) -> Vec<Detection> {
    detections.sort_by(|a, b| {
        b.confidence
            .partial_cmp(&a.confidence)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then((b.end - b.start).cmp(&(a.end - a.start)))
            .then(a.start.cmp(&b.start))
    });

    let mut accepted: Vec<Detection> = Vec::new();
    for d in detections {
        if accepted.iter().any(|a| overlaps(a, &d)) {
            continue;
        }
        accepted.push(d);
    }
    accepted.sort_by_key(|d| d.start);
    accepted
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::category;

    fn det(start: usize, end: usize, confidence: f64, rule_id: &'static str) -> Detection {
        Detection {
            start,
            end,
            category: category::SECRET,
            confidence,
            rule_id,
        }
    }

    #[test]
    fn higher_confidence_wins_over_overlapping_lower_confidence() {
        let dets = vec![det(0, 10, 0.6, "weak"), det(2, 8, 0.99, "strong")];
        let resolved = resolve_overlaps(dets);
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].rule_id, "strong");
    }

    #[test]
    fn longer_span_wins_when_confidence_tied() {
        let dets = vec![det(0, 5, 0.9, "short"), det(0, 10, 0.9, "long")];
        let resolved = resolve_overlaps(dets);
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].rule_id, "long");
    }

    #[test]
    fn non_overlapping_spans_are_both_kept_sorted_by_start() {
        let dets = vec![det(20, 30, 0.9, "second"), det(0, 10, 0.9, "first")];
        let resolved = resolve_overlaps(dets);
        assert_eq!(resolved.len(), 2);
        assert_eq!(resolved[0].rule_id, "first");
        assert_eq!(resolved[1].rule_id, "second");
    }
}

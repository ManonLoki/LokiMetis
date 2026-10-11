//! 今日摘要只格式化 core 资格投影，不在视图推导可用性或查询来源。

use loki_metris_core::{Completeness, Confidence, TodayUsageDisplay};

/// 窄窗保留短值与质量标记；可访问名称和悬停始终提供精确整数及完整质量。
pub(super) fn labels(display: TodayUsageDisplay, locale: &str, narrow: bool) -> (String, String) {
    let t = |key: &str| rust_i18n::t!(key, locale = locale).to_string();
    let (value, exact, quality) = match display {
        TodayUsageDisplay::Value {
            total_tokens,
            completeness,
            confidence,
        } => {
            let mut quality = Vec::new();
            match completeness {
                Completeness::Complete => {}
                Completeness::Partial => quality.push(t("monitor.pet.todayPartial")),
                Completeness::Unknown => quality.push(t("monitor.pet.todayUnknownQuality")),
            }
            match confidence {
                Confidence::Exact => {}
                Confidence::Derived => quality.push(t("monitor.pet.todayDerived")),
                Confidence::Suspected => quality.push(t("monitor.pet.todaySuspected")),
            }
            (
                crate::format_tokens::compact(total_tokens),
                crate::format_tokens::exact(total_tokens),
                quality.join(" · "),
            )
        }
        other => {
            let key = match other {
                TodayUsageDisplay::Pending => "monitor.pet.todayPending",
                TodayUsageDisplay::NoRecords => "monitor.pet.todayNoRecords",
                TodayUsageDisplay::Unavailable => "monitor.pet.todayUnavailable",
                TodayUsageDisplay::Expired => "monitor.pet.todayExpired",
                TodayUsageDisplay::Value { .. } => unreachable!(),
            };
            let text = t(key);
            (text.clone(), text, String::new())
        }
    };
    let quality = if quality.is_empty() {
        String::new()
    } else {
        format!(" · {quality}")
    };
    let full = t("monitor.pet.todayAria")
        .replace("%{value}", &exact)
        .replace("%{quality}", &quality);
    let visible = if narrow {
        format!("{value}{}", if quality.is_empty() { "" } else { "*" })
    } else {
        t("monitor.pet.todayVisible")
            .replace("%{value}", &value)
            .replace("%{quality}", &quality)
    };
    (visible, full)
}

//! 看板展示格式与参考产品一致，整数运算避免大 Token 的浮点精度损失。

/// 十进制每三位分组，只影响展示，复制仍使用原始精确整数。
pub(super) fn exact(value: u64) -> String {
    let digits = value.to_string();
    let mut output = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) { output.push(','); }
        output.push(digit);
    }
    output
}

/// K/M/B 固定两位小数；四舍五入跨 1000 时晋级，最大单位仍加千分位。
pub(super) fn compact(value: u64) -> String {
    if value < 1_000 { return exact(value) }
    let units = [(1_000_u128, "K"), (1_000_000, "M"), (1_000_000_000, "B")];
    let mut index = units.iter().rposition(|(threshold, _)| u128::from(value) >= *threshold).unwrap_or(0);
    let mut rounded = (u128::from(value) * 100 + units[index].0 / 2) / units[index].0;
    if rounded >= 100_000 && index < units.len() - 1 {
        index += 1;
        rounded = (u128::from(value) * 100 + units[index].0 / 2) / units[index].0;
    }
    format!("{}.{:02}{}", exact((rounded / 100) as u64), rounded % 100, units[index].1)
}

/// 观测时间始终使用设备时区，异常时间保留明确占位。
pub(super) fn observed(epoch_ms: i64, locale: &str) -> String {
    let Ok(timestamp) = jiff::Timestamp::from_millisecond(epoch_ms) else { return "—".into() };
    let zoned = timestamp.to_zoned(jiff::tz::TimeZone::system());
    let date = calendar(&zoned.date().to_string(), locale);
    if locale == "zh-CN" { format!("{date} {}",zoned.strftime("%H:%M")) }
    else { format!("{date}, {}:{:02} {}", match zoned.hour()%12 {0=>12,h=>h},zoned.minute(),if zoned.hour()<12 {"AM"}else{"PM"}) }
}

/// 自然日标签不经过时区换算，避免午夜日期发生偏移。
pub(super) fn calendar(value: &str, locale: &str) -> String {
    let Ok(date) = value.parse::<jiff::civil::Date>() else { return if locale=="zh-CN" {"未知"}else{"Unknown"}.into() };
    if locale == "zh-CN" { format!("{}年{}月{}日",date.year(),date.month(),date.day()) }
    else { format!("{} {}, {}",["Jan","Feb","Mar","Apr","May","Jun","Jul","Aug","Sep","Oct","Nov","Dec"][date.month() as usize-1],date.day(),date.year()) }
}

/// 积分以两位小数和千分位显示；不再把显示字符串用于聚合。
pub(super) fn credits(value:f64)->String {
    let formatted=format!("{value:.2}");
    let Some((whole,fraction))=formatted.split_once('.') else {return formatted};
    let (sign,digits)=whole.strip_prefix('-').map_or(("",whole),|digits|("-",digits));
    let mut grouped=sign.to_owned();
    for(index,digit)in digits.chars().enumerate(){if index>0 && (digits.len()-index).is_multiple_of(3){grouped.push(',');}grouped.push(digit);}
    format!("{grouped}.{fraction}")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 半入、跨单位和 u64 上界保持精确且与参考小数规则一致。
    #[test]
    fn token_format_preserves_rounding_and_large_integers() {
        assert_eq!(exact(73_461_934), "73,461,934");
        assert_eq!(compact(999), "999");
        assert_eq!(compact(1_005), "1.01K");
        assert_eq!(compact(999_995), "1.00M");
        assert_eq!(compact(73_461_934), "73.46M");
        assert_eq!(compact(u64::MAX), "18,446,744,073.71B");
        assert_eq!(credits(1234.567),"1,234.57");
        assert_eq!(credits(-123.45),"-123.45");
        assert_eq!(credits(-1234.5),"-1,234.50");
    }

    /// 日历格式匹配原版 Intl medium，非法日不发生静默进位。
    #[test]
    fn calendar_localizes_and_rejects_invalid_day() {
        assert_eq!(calendar("2026-10-09","zh-CN"),"2026年10月9日");
        assert_eq!(calendar("2026-10-09","en-US"),"Oct 9, 2026");
        assert_eq!(calendar("2026-02-30","zh-CN"),"未知");
    }
}

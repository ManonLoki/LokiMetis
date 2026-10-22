//! 日期、模型与整个查询范围共享同一事件集合和整数求和规则。

use std::collections::BTreeMap;
use jiff::tz::TimeZone;
use crate::{TimeStandard, civil_date_for_timestamp};
use super::{CursorUsageError as Error, CursorUsagePages, CursorUsageSummary, CursorTokenCounts, CursorUsageGroup};

impl CursorTokenCounts {
    /// 将单事件的已报告分项求和，所有缺失分项仍独立计数。
    fn add(&mut self, event: &super::pages::Event) -> Result<(), Error> {
        self.events += 1;
        self.without_usage += u64::from(event.without_usage);
        for (field, value) in [&mut self.input, &mut self.output, &mut self.cache_read, &mut self.cache_write]
            .into_iter().zip(event.tokens) {
            if let Some(value) = value {
                field.tokens = field.tokens.checked_add(value).ok_or(Error::Overflow)?;
                field.reported += 1;
            } else { field.missing += 1; }
        }
        Ok(())
    }
}

impl CursorUsagePages {
    /// 发布前再次核对完整性；模型与日期不改变原 API 字段的口径。
    pub fn finish(self, observed_at_ms: i64, zone: &TimeZone) -> Result<CursorUsageSummary, Error> {
        if self.next_page().is_some() { return Err(Error::Incomplete) }
        let mut counts = CursorTokenCounts::default();
        let mut daily = BTreeMap::new();
        let mut models = BTreeMap::new();
        let first = civil_date_for_timestamp(self.range.start_ms, &TimeStandard::Local, zone).ok_or(Error::Malformed)?;
        let last = civil_date_for_timestamp(self.range.end_ms - 1, &TimeStandard::Local, zone).ok_or(Error::Malformed)?;
        let mut date = first;
        while date <= last {
            daily.insert(Some(date.to_string()), CursorTokenCounts::default());
            date = date.checked_add(jiff::Span::new().days(1)).map_err(|_| Error::Malformed)?;
        }
        for event in self.events.values() {
            counts.add(event)?;
            let date = civil_date_for_timestamp(event.timestamp, &TimeStandard::Local, zone).ok_or(Error::Malformed)?;
            daily.entry(Some(date.to_string())).or_insert_with(CursorTokenCounts::default).add(event)?;
            models.entry(event.model.clone()).or_insert_with(CursorTokenCounts::default).add(event)?;
        }
        let groups = |map: BTreeMap<Option<String>, CursorTokenCounts>| map.into_iter()
            .map(|(label, counts)| CursorUsageGroup { label, counts }).collect();
        Ok(CursorUsageSummary { range: self.range, observed_at_ms, counts, daily: groups(daily), models: groups(models) })
    }
}

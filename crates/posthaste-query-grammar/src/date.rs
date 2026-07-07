use super::nodes::condition_node;
use super::*;

pub(super) fn date_node(value: &str, negated: bool) -> Result<Vec<MailQueryRuleNode>, String> {
    let date = time::Date::parse(
        value,
        &time::format_description::parse("[year]-[month]-[day]")
            .map_err(|e| format!("date format error: {e}"))?,
    )
    .map_err(|e| format!("invalid date '{value}': {e}"))?;

    let start = date
        .midnight()
        .assume_utc()
        .format(&Rfc3339)
        .map_err(|e| format!("date format error: {e}"))?;

    let next_day = date.next_day().ok_or_else(|| "date overflow".to_string())?;
    let end = next_day
        .midnight()
        .assume_utc()
        .format(&Rfc3339)
        .map_err(|e| format!("date format error: {e}"))?;

    Ok(vec![MailQueryRuleNode::Group(MailQueryGroup {
        operator: MailQueryGroupOperator::All,
        negated,
        nodes: vec![
            MailQueryRuleNode::Condition(MailQueryCondition {
                field: MailQueryField::ReceivedAt,
                operator: MailQueryOperator::Ge,
                negated: false,
                value: MailQueryValue::String(start),
            }),
            MailQueryRuleNode::Condition(MailQueryCondition {
                field: MailQueryField::ReceivedAt,
                operator: MailQueryOperator::Lt,
                negated: false,
                value: MailQueryValue::String(end),
            }),
        ],
    })])
}

/// `newer:7d` / `older:2w` — relative date from now.
pub(super) fn relative_date_node(
    value: &str,
    operator: MailQueryOperator,
    negated: bool,
) -> Result<Vec<MailQueryRuleNode>, String> {
    let iso = compute_relative_date(value)?;
    Ok(vec![condition_node(
        MailQueryField::ReceivedAt,
        operator,
        MailQueryValue::String(iso),
        negated,
    )])
}

/// Parses `Nd`, `Nw`, `Nm`, `Ny` and subtracts from now.
fn compute_relative_date(spec: &str) -> Result<String, String> {
    if spec.len() < 2 {
        return Err(format!("invalid relative date: {spec}"));
    }
    let (num_str, unit) = spec.split_at(spec.len() - 1);
    let n: i64 = num_str
        .parse()
        .map_err(|_| format!("invalid number in relative date: {num_str}"))?;

    let now = OffsetDateTime::now_utc();
    let target = match unit {
        "d" => now - Duration::days(n),
        "w" => now - Duration::weeks(n),
        "m" => now - Duration::days(n * 30),
        "y" => now - Duration::days(n * 365),
        _ => return Err(format!("unknown relative date unit: {unit}")),
    };
    target
        .format(&Rfc3339)
        .map_err(|e| format!("date format error: {e}"))
}

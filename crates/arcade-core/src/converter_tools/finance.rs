//! Everyday money maths: percentages, discounts, VAT/GST, tips, and loan EMIs.

use super::format_number;
use crate::tool_kit::{number_in, option_str};
use arcade_contract::ToolRequest;
use serde_json::{Value, json};

const MAX_AMOUNT: f64 = 1e15;

use super::format_money as money;

fn amount(request: &ToolRequest, key: &str, label: &str) -> Result<f64, String> {
    number_in(request, key, label, None, -MAX_AMOUNT..=MAX_AMOUNT)
}

fn percent(request: &ToolRequest, key: &str, label: &str) -> Result<f64, String> {
    number_in(request, key, label, None, -1_000_000.0..=1_000_000.0)
}

pub(super) fn percentage(request: &ToolRequest) -> Result<Value, String> {
    match option_str(request, "mode", "of") {
        "of" => {
            let rate = percent(request, "percent", "the percentage")?;
            let value = amount(request, "value", "the number")?;
            let result = value * rate / 100.0;
            Ok(json!({
                "headline": format_number(round(result)),
                "summary": format!("{}% of {} is {}", format_number(rate), format_number(value), format_number(round(result))),
                "result": result,
            }))
        }
        "ratio" => {
            let part = amount(request, "part", "the part")?;
            let whole = amount(request, "value", "the whole")?;
            if whole == 0.0 {
                return Err("The whole cannot be zero".into());
            }
            let result = part / whole * 100.0;
            Ok(json!({
                "headline": format!("{}%", format_number(round(result))),
                "summary": format!("{} is {}% of {}", format_number(part), format_number(round(result)), format_number(whole)),
                "result": result,
            }))
        }
        "change" => {
            let from = amount(request, "from", "the old value")?;
            let to = amount(request, "to", "the new value")?;
            if from == 0.0 {
                return Err("The old value cannot be zero".into());
            }
            let change = (to - from) / from.abs() * 100.0;
            let direction = if change >= 0.0 {
                "increase"
            } else {
                "decrease"
            };
            Ok(json!({
                "headline": format!("{}{}%", if change >= 0.0 { "+" } else { "−" }, format_number(round(change.abs()))),
                "summary": format!("From {} to {} is a {}% {direction}", format_number(from), format_number(to), format_number(round(change.abs()))),
                "difference": to - from,
                "result": change,
            }))
        }
        "discount" => {
            let price = amount(request, "price", "the price")?;
            let off = number_in(request, "discount", "the discount", None, 0.0..=100.0)?;
            let saved = price * off / 100.0;
            Ok(json!({
                "headline": money(price - saved),
                "summary": format!("{}% off {} saves {}", format_number(off), money(price), money(saved)),
                "finalPrice": price - saved,
                "youSave": saved,
            }))
        }
        "tax" => {
            let price = amount(request, "price", "the price")?;
            let rate = number_in(request, "rate", "the tax rate", None, 0.0..=1000.0)?;
            if option_str(request, "taxMode", "add") == "remove" {
                let net = price / (1.0 + rate / 100.0);
                Ok(json!({
                    "headline": money(net),
                    "summary": format!("{} includes {} tax at {}%", money(price), money(price - net), format_number(rate)),
                    "priceBeforeTax": net,
                    "tax": price - net,
                    "priceWithTax": price,
                }))
            } else {
                let tax = price * rate / 100.0;
                Ok(json!({
                    "headline": money(price + tax),
                    "summary": format!("{} plus {}% tax ({}) is {}", money(price), format_number(rate), money(tax), money(price + tax)),
                    "priceBeforeTax": price,
                    "tax": tax,
                    "priceWithTax": price + tax,
                }))
            }
        }
        "tip" => {
            let bill = number_in(request, "bill", "the bill", None, 0.0..=MAX_AMOUNT)?;
            let tip_rate = number_in(request, "tip", "the tip", Some(0.0), 0.0..=100.0)?;
            let people = number_in(
                request,
                "people",
                "the number of people",
                Some(1.0),
                1.0..=1000.0,
            )?
            .round();
            let tip = bill * tip_rate / 100.0;
            let total = bill + tip;
            Ok(json!({
                "headline": if people > 1.0 { format!("{} each", money(total / people)) } else { money(total) },
                "summary": format!("{} tip on {} makes {}{}", money(tip), money(bill), money(total), if people > 1.0 { format!(", split {people} ways") } else { String::new() }),
                "tip": tip,
                "total": total,
                "perPerson": total / people,
                "tipPerPerson": tip / people,
            }))
        }
        other => Err(format!("Unknown calculation `{other}`")),
    }
}

fn round(value: f64) -> f64 {
    (value * 10_000.0).round() / 10_000.0
}

pub(super) fn loan(request: &ToolRequest) -> Result<Value, String> {
    let principal = number_in(
        request,
        "principal",
        "the loan amount",
        None,
        1.0..=MAX_AMOUNT,
    )?;
    let annual_rate = number_in(request, "rate", "the interest rate", None, 0.0..=100.0)?;
    let term = number_in(request, "term", "the loan term", None, 1.0..=1200.0)?.round();
    let months = if option_str(request, "termUnit", "years") == "months" {
        term
    } else {
        term * 12.0
    };
    if months > 1200.0 {
        return Err("Loan term must be 100 years or less".into());
    }
    let monthly_rate = annual_rate / 1200.0;
    let emi = if monthly_rate == 0.0 {
        principal / months
    } else {
        let growth = (1.0 + monthly_rate).powf(months);
        principal * monthly_rate * growth / (growth - 1.0)
    };
    // Year-by-year schedule; the last payment absorbs rounding drift.
    let mut balance = principal;
    let mut schedule = Vec::new();
    let (mut year_principal, mut year_interest) = (0.0, 0.0);
    for month in 1..=months as u32 {
        let interest = balance * monthly_rate;
        let repaid = if month == months as u32 {
            balance
        } else {
            (emi - interest).min(balance)
        };
        balance -= repaid;
        year_principal += repaid;
        year_interest += interest;
        if month % 12 == 0 || month == months as u32 {
            schedule.push(json!({
                "year": month.div_ceil(12),
                "principalPaid": money(year_principal),
                "interestPaid": money(year_interest),
                "balance": money(balance.max(0.0)),
            }));
            year_principal = 0.0;
            year_interest = 0.0;
        }
    }
    let total = emi * months;
    Ok(json!({
        "headline": format!("{} per month", money(emi)),
        "summary": format!("{} payments of {}; total interest {}", months, money(emi), money(total - principal)),
        "monthlyPayment": money(emi),
        "totalInterest": money(total - principal),
        "totalPayment": money(total),
        "interestShare": format!("{:.1}%", (total - principal) / total * 100.0),
        "schedule": schedule,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(options: Value) -> ToolRequest {
        ToolRequest {
            tool_id: "test".into(),
            inputs: vec![],
            options,
        }
    }

    #[test]
    fn percentage_modes_cover_everyday_cases() {
        let value = percentage(&request(json!({"mode":"of","percent":15,"value":200}))).unwrap();
        assert_eq!(value["headline"], "30");
        let value = percentage(&request(json!({"mode":"change","from":80,"to":100}))).unwrap();
        assert_eq!(value["headline"], "+25%");
        let value = percentage(&request(
            json!({"mode":"tax","price":118,"rate":18,"taxMode":"remove"}),
        ))
        .unwrap();
        assert_eq!(value["headline"], "100");
        let value = percentage(&request(
            json!({"mode":"tip","bill":100,"tip":10,"people":4}),
        ))
        .unwrap();
        assert_eq!(value["headline"], "27.50 each");
    }

    #[test]
    fn loan_emi_matches_the_standard_formula() {
        let value = loan(&request(json!({"principal":1000000,"rate":8.5,"term":20}))).unwrap();
        assert_eq!(value["monthlyPayment"], "8,678.23");
        assert_eq!(value["schedule"].as_array().unwrap().len(), 20);
        assert_eq!(value["schedule"][19]["balance"], "0");
        let value = loan(&request(
            json!({"principal":1200,"rate":0,"term":12,"termUnit":"months"}),
        ))
        .unwrap();
        assert_eq!(value["monthlyPayment"], "100");
    }
}

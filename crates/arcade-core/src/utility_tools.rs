//! Everyday quick utilities: emoji search and random picks. The timer and
//! stopwatch run entirely in the interface.

use crate::tool_kit::{
    check_cancelled, json_result, number_in, option_bool, option_str, single_value,
};
use arcade_contract::{ToolManifest, ToolRequest, ToolResult};
use emojis::Group;
use rand::{Rng, seq::SliceRandom};
use serde_json::{Value, json};
use std::sync::atomic::AtomicBool;

const INPUT_LIMIT: usize = 256 * 1024;
const MAX_EMOJI_RESULTS: usize = 400;

pub fn execute(
    manifest: &ToolManifest,
    request: &ToolRequest,
    cancelled: &AtomicBool,
) -> Result<ToolResult, String> {
    check_cancelled(cancelled)?;
    let text = single_value(request, INPUT_LIMIT)?;
    match manifest.id.as_str() {
        "arcade.utility.emoji" => Ok(json_result(
            manifest,
            emoji_search(request, text)?,
            "structured/emoji-list",
        )),
        "arcade.utility.random" => Ok(json_result(
            manifest,
            random(request, text)?,
            "structured/random",
        )),
        "arcade.utility.timer" => Err("The timer runs in the Arcade Box window".into()),
        _ => Err(format!(
            "No utility executor is registered for {}",
            manifest.id
        )),
    }
}

const GROUPS: &[(&str, Group, &str)] = &[
    ("smileys", Group::SmileysAndEmotion, "Smileys & emotion"),
    ("people", Group::PeopleAndBody, "People & body"),
    ("animals", Group::AnimalsAndNature, "Animals & nature"),
    ("food", Group::FoodAndDrink, "Food & drink"),
    ("travel", Group::TravelAndPlaces, "Travel & places"),
    ("activities", Group::Activities, "Activities"),
    ("objects", Group::Objects, "Objects"),
    ("symbols", Group::Symbols, "Symbols"),
    ("flags", Group::Flags, "Flags"),
];

fn emoji_search(request: &ToolRequest, query: &str) -> Result<Value, String> {
    let group = match option_str(request, "group", "all") {
        "all" => None,
        key => Some(
            GROUPS
                .iter()
                .find(|(name, _, _)| *name == key)
                .map(|(_, group, _)| *group)
                .ok_or_else(|| format!("Unknown emoji group `{key}`"))?,
        ),
    };
    let query = query.trim().to_lowercase();
    let terms = query.split_whitespace().collect::<Vec<_>>();
    let mut found = emojis::iter()
        .filter(|emoji| group.is_none_or(|group| emoji.group() == group))
        .filter_map(|emoji| {
            let score = if terms.is_empty() {
                0
            } else {
                emoji_score(emoji, &query, &terms)?
            };
            Some((score, emoji))
        })
        .collect::<Vec<_>>();
    // Stable sort keeps Unicode order within a score.
    found.sort_by_key(|(score, _)| *score);
    let total = found.len();
    let label = |emoji: &emojis::Emoji| {
        GROUPS
            .iter()
            .find(|(_, group, _)| *group == emoji.group())
            .map_or("", |(_, _, label)| label)
    };
    let items = found
        .into_iter()
        .take(MAX_EMOJI_RESULTS)
        .map(|(_, emoji)| {
            json!({
                "emoji": emoji.as_str(),
                "name": emoji.name(),
                "shortcode": emoji.shortcode().map(|code| format!(":{code}:")),
                "group": label(emoji),
            })
        })
        .collect::<Vec<_>>();
    Ok(json!({
        "query": query,
        "count": total,
        "shown": items.len(),
        "emojis": items,
    }))
}

/// Lower is better; `None` means no match. Every term must match a word of
/// the name or shortcode.
fn emoji_score(emoji: &emojis::Emoji, query: &str, terms: &[&str]) -> Option<u8> {
    let name = emoji.name().to_lowercase();
    let codes = emoji
        .shortcodes()
        .collect::<Vec<_>>()
        .join(" ")
        .replace('_', " ");
    if name == query || emoji.shortcodes().any(|code| code == query) {
        return Some(0);
    }
    let words = name
        .split(|ch: char| !ch.is_alphanumeric())
        .chain(codes.split_whitespace())
        .collect::<Vec<_>>();
    let all_prefix = terms
        .iter()
        .all(|term| words.iter().any(|word| word.starts_with(term)));
    if all_prefix {
        return Some(if name.starts_with(query) { 1 } else { 2 });
    }
    terms
        .iter()
        .all(|term| name.contains(term) || codes.contains(term))
        .then_some(3)
}

fn random(request: &ToolRequest, text: &str) -> Result<Value, String> {
    let mut rng = rand::rng();
    let lines = || {
        let items = text
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(str::to_owned)
            .collect::<Vec<_>>();
        if items.is_empty() {
            Err("Type the choices in the box, one per line".to_owned())
        } else {
            Ok(items)
        }
    };
    let count = |default: f64, max: f64| {
        number_in(request, "count", "how many", Some(default), 1.0..=max)
            .map(|value| value.round() as usize)
    };
    match option_str(request, "mode", "pick") {
        "pick" => {
            let mut items = lines()?;
            let wanted = count(1.0, 10_000.0)?;
            let picks = if option_bool(request, "repeat", false) {
                (0..wanted)
                    .map(|_| items[rng.random_range(0..items.len())].clone())
                    .collect::<Vec<_>>()
            } else {
                if wanted > items.len() {
                    return Err(format!("Only {} choices to pick from", items.len()));
                }
                items.shuffle(&mut rng);
                items.truncate(wanted);
                items
            };
            Ok(json!({"headline": picks.join(", "), "results": picks}))
        }
        "shuffle" => {
            let mut items = lines()?;
            items.shuffle(&mut rng);
            Ok(json!({"headline": format!("Shuffled {} items", items.len()), "results": items}))
        }
        "teams" => {
            let mut items = lines()?;
            let teams = number_in(
                request,
                "teams",
                "the number of teams",
                Some(2.0),
                2.0..=100.0,
            )?
            .round() as usize;
            if teams > items.len() {
                return Err(format!("Need at least {teams} names to make {teams} teams"));
            }
            items.shuffle(&mut rng);
            let mut groups = vec![Vec::new(); teams];
            for (index, item) in items.into_iter().enumerate() {
                groups[index % teams].push(item);
            }
            let results = groups
                .iter()
                .enumerate()
                .map(|(index, members)| json!({"team": format!("Team {}", index + 1), "members": members.join(", ")}))
                .collect::<Vec<_>>();
            Ok(json!({"headline": format!("{teams} teams"), "results": results}))
        }
        "number" => {
            let min = number_in(request, "min", "the minimum", Some(1.0), -1e15..=1e15)?;
            let max = number_in(request, "max", "the maximum", Some(100.0), -1e15..=1e15)?;
            let (min, max) = (min.round() as i64, max.round() as i64);
            if min > max {
                return Err("The minimum must not be larger than the maximum".into());
            }
            let wanted = count(1.0, 10_000.0)?;
            let unique = !option_bool(request, "repeat", false);
            let span = max.abs_diff(min).saturating_add(1);
            if unique && wanted as u64 > span {
                return Err(format!(
                    "There are only {span} different numbers in that range"
                ));
            }
            let mut picks = Vec::with_capacity(wanted);
            while picks.len() < wanted {
                let value = rng.random_range(min..=max);
                if !unique || !picks.contains(&value) {
                    picks.push(value);
                }
            }
            let headline = picks
                .iter()
                .map(i64::to_string)
                .collect::<Vec<_>>()
                .join(", ");
            Ok(json!({"headline": headline, "results": picks}))
        }
        "coin" => {
            let flips = count(1.0, 1000.0)?;
            let results = (0..flips)
                .map(|_| {
                    if rng.random_bool(0.5) {
                        "Heads"
                    } else {
                        "Tails"
                    }
                })
                .collect::<Vec<_>>();
            let heads = results.iter().filter(|side| **side == "Heads").count();
            let headline = if flips == 1 {
                results[0].to_owned()
            } else {
                format!("{heads} heads, {} tails", flips - heads)
            };
            Ok(json!({"headline": headline, "results": results}))
        }
        "dice" => {
            let dice = count(1.0, 100.0)?;
            let sides = number_in(
                request,
                "sides",
                "the number of sides",
                Some(6.0),
                2.0..=1000.0,
            )?
            .round() as u32;
            let rolls = (0..dice)
                .map(|_| rng.random_range(1..=sides))
                .collect::<Vec<_>>();
            let total: u32 = rolls.iter().sum();
            let headline = if dice == 1 {
                total.to_string()
            } else {
                format!("Total {total}")
            };
            Ok(json!({"headline": headline, "results": rolls, "total": total}))
        }
        other => Err(format!("Unknown random mode `{other}`")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(options: Value) -> ToolRequest {
        ToolRequest {
            tool_id: "t".into(),
            inputs: vec![],
            options,
        }
    }

    #[test]
    fn emoji_search_ranks_name_matches_first() {
        let value = emoji_search(&request(json!({})), "thumbs up").unwrap();
        assert_eq!(value["emojis"][0]["emoji"], "👍");
        let value = emoji_search(&request(json!({"group": "flags"})), "india").unwrap();
        assert_eq!(value["emojis"][0]["emoji"], "🇮🇳");
        let value = emoji_search(&request(json!({})), "").unwrap();
        assert_eq!(value["shown"], MAX_EMOJI_RESULTS);
    }

    #[test]
    fn random_modes_respect_limits() {
        let value = random(&request(json!({"mode": "pick", "count": 2})), "a\nb\nc").unwrap();
        assert_eq!(value["results"].as_array().unwrap().len(), 2);
        assert!(random(&request(json!({"mode": "pick", "count": 5})), "a\nb").is_err());
        let value = random(
            &request(json!({"mode": "number", "min": 1, "max": 3, "count": 3})),
            "",
        )
        .unwrap();
        let mut numbers = value["results"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_i64().unwrap())
            .collect::<Vec<_>>();
        numbers.sort();
        assert_eq!(numbers, vec![1, 2, 3]);
        let value = random(
            &request(json!({"mode": "teams", "teams": 2})),
            "a\nb\nc\nd\ne",
        )
        .unwrap();
        assert_eq!(value["results"].as_array().unwrap().len(), 2);
    }
}
